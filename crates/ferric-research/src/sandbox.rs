//! Executing retrieval commands inside a hardened container sandbox.
//!
//! **The airlock is opt-out, not opt-in (ADR-074).** The previous default paired
//! `--network bridge` with no proxy and no gVisor, so the "sandbox" a caller got
//! by writing `SandboxConfig::default()` had dropped capabilities and
//! *unrestricted network egress* — for a component whose entire job is running
//! untrusted retrieval. The allowlist proxy and the gVisor runtime were both
//! knobs nothing in the tree ever set.
//!
//! Now the default denies the network outright, and letting traffic out
//! unfiltered requires naming [`NetworkPolicy::Unrestricted`]. A reviewer
//! grepping for that name finds every place egress is possible.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox exec failed: {0}")]
    Exec(String),
    #[error("docker daemon not reachable")]
    NotAvailable,
}

/// What the sandboxed process may reach.
///
/// This is an enum rather than an `Option<proxy_url>` so that "unrestricted" is
/// a thing someone had to *write*, not a thing they got by leaving a field
/// unset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// No network. The default.
    Denied,
    /// Egress only through an allowlist gateway, on a docker network created
    /// with `--internal` so the sandbox has **no route out except the gateway**.
    ///
    /// This replaces a `Proxy(url)` variant that set `http_proxy` on a **bridge**
    /// network (ADR-082). That was advisory, not enforced: proxy environment
    /// variables are a convention cooperative clients honour, and a container
    /// could simply `unset http_proxy` and reach the internet directly —
    /// measured, fetching example.com in full. The isolation has to come from
    /// the network, not from the client's goodwill.
    Airlock {
        /// An `--internal` docker network the sandbox is attached to.
        network: String,
        /// The gateway's proxy URL, reachable only on that network.
        proxy_url: String,
    },
    /// Unrestricted egress. Deliberately verbose to write.
    Unrestricted,
}

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub image: String,
    /// Require the gVisor runtime. On by default: if `runsc` is missing the run
    /// fails closed, which is the correct direction for a security control.
    pub enforce_runsc: bool,
    pub network: NetworkPolicy,
}

impl Default for SandboxConfig {
    /// The airlock, fully closed.
    fn default() -> Self {
        Self {
            image: "alpine:latest".to_string(),
            enforce_runsc: true,
            network: NetworkPolicy::Denied,
        }
    }
}

impl SandboxConfig {
    /// Opt *out* of the airlock — no gVisor, unrestricted egress. Exists so the
    /// unsafe configuration has exactly one obvious name to grep for.
    pub fn unrestricted_for_testing() -> Self {
        Self {
            image: "alpine:latest".to_string(),
            enforce_runsc: false,
            network: NetworkPolicy::Unrestricted,
        }
    }
}

/// How long to wait for `docker info` before calling the daemon unavailable.
///
/// A *half-started* Docker Desktop is the case that matters: the CLI is present
/// and the daemon is not, and `docker info` then **hangs** rather than failing.
/// Observed on this machine at ~60 s per call (ADR-081) — long enough that a
/// caller looks wedged, and long enough that a test suite gating on
/// availability silently spends minutes doing nothing.
const AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(5);

/// Is a Docker daemon reachable right now?
///
/// Bounded: an unreachable daemon must answer "no" quickly, not block the
/// caller. `Retriever::available()` sits on the research path, so an
/// unbounded probe here would stall a whole run over an optional dependency.
pub fn check_available() -> bool {
    let Ok(mut child) = Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false; // no docker binary at all
    };

    let deadline = Instant::now() + AVAILABILITY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Half-started daemon: reap the probe so it cannot linger.
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return false,
        }
    }
}

/// Build the full `docker` argument vector for a sandboxed run.
///
/// Split out from [`run_in_sandbox`] so the security-relevant part — which flags
/// actually reach `docker` — is testable without a Docker daemon present. That
/// matters: this is the code most likely to be wrong and least likely to be
/// exercised, since nothing in the binary reaches it yet.
pub fn docker_args(config: &SandboxConfig, cmd: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = vec!["run".into(), "--rm".into()];

    match &config.network {
        NetworkPolicy::Denied => {
            args.push("--network".into());
            args.push("none".into());
        }
        NetworkPolicy::Airlock { network, proxy_url } => {
            // The isolated network is what enforces this; the env vars only
            // point cooperative clients at the gateway. Note this is NOT
            // `bridge` — attaching to bridge would restore the bypass.
            args.push("--network".into());
            args.push(network.clone());
            args.push("--env".into());
            args.push(format!("http_proxy={proxy_url}"));
            args.push("--env".into());
            args.push(format!("https_proxy={proxy_url}"));
        }
        NetworkPolicy::Unrestricted => {
            args.push("--network".into());
            args.push("bridge".into());
        }
    }

    args.push("--cap-drop=ALL".into());
    args.push("--security-opt".into());
    args.push("no-new-privileges".into());

    if config.enforce_runsc {
        args.push("--runtime=runsc".into());
    }

    args.push(config.image.clone());
    args.extend(cmd.iter().map(|s| s.to_string()));
    args
}

pub fn run_in_sandbox(config: &SandboxConfig, cmd: &[&str]) -> Result<String, SandboxError> {
    if !check_available() {
        return Err(SandboxError::NotAvailable);
    }

    let output = Command::new("docker")
        .args(docker_args(config, cmd))
        .output()
        .map_err(|e| SandboxError::Exec(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SandboxError::Exec(format!(
            "exit code {}: {}",
            output.status, stderr
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(config: &SandboxConfig) -> Vec<String> {
        docker_args(config, &["echo", "hi"])
    }

    /// The regression this module exists for: the default must not be able to
    /// reach the network.
    #[test]
    fn the_default_denies_the_network() {
        let args = args_of(&SandboxConfig::default());
        let joined = args.join(" ");
        assert!(
            joined.contains("--network none"),
            "default must deny egress, got: {joined}"
        );
        assert!(
            !joined.contains("--network bridge"),
            "default must not attach a bridge, got: {joined}"
        );
    }

    /// The gVisor runtime is part of the airlock, so it is on by default too.
    /// If `runsc` is absent the run fails closed, which is correct.
    #[test]
    fn the_default_requires_gvisor() {
        assert!(args_of(&SandboxConfig::default()).contains(&"--runtime=runsc".to_string()));
    }

    /// Capability drops applied regardless of network policy — these were the
    /// one part the old default got right.
    #[test]
    fn capabilities_are_always_dropped() {
        for config in [
            SandboxConfig::default(),
            SandboxConfig::unrestricted_for_testing(),
        ] {
            let args = args_of(&config);
            assert!(args.contains(&"--cap-drop=ALL".to_string()));
            assert!(args.contains(&"no-new-privileges".to_string()));
        }
    }

    /// The airlock attaches to the ISOLATED network and points clients at the
    /// gateway. The critical assertion is the negative one: it must never say
    /// `bridge`, because that is what made the old `Proxy` variant bypassable.
    #[test]
    fn the_airlock_uses_the_isolated_network_not_bridge() {
        let config = SandboxConfig {
            network: NetworkPolicy::Airlock {
                network: "ferric-airlock".into(),
                proxy_url: "http://172.19.0.2:8888".into(),
            },
            ..SandboxConfig::default()
        };
        let joined = args_of(&config).join(" ");
        assert!(joined.contains("--network ferric-airlock"));
        assert!(
            !joined.contains("--network bridge"),
            "bridge would restore the `unset http_proxy` bypass: {joined}"
        );
        assert!(joined.contains("http_proxy=http://172.19.0.2:8888"));
        assert!(joined.contains("https_proxy=http://172.19.0.2:8888"));
    }

    /// Unrestricted egress is reachable — but only by naming it.
    #[test]
    fn unrestricted_is_available_but_must_be_named() {
        let joined = args_of(&SandboxConfig::unrestricted_for_testing()).join(" ");
        assert!(joined.contains("--network bridge"));
        assert!(!joined.contains("http_proxy"));
    }

    /// The command lands after the image, so it cannot be read as a docker flag.
    #[test]
    fn the_command_follows_the_image() {
        let args = docker_args(&SandboxConfig::default(), &["sh", "-c", "echo hi"]);
        let image_at = args.iter().position(|a| a == "alpine:latest").unwrap();
        assert_eq!(&args[image_at + 1..], &["sh", "-c", "echo hi"]);
    }
}
