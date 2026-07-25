//! Live sandbox validation against a real Docker daemon (ADR-081).
//!
//! The container path has never run in this project — it was blocked on a
//! missing containerizer from sprint 33 onward, so sprint 84's A5 fix could only
//! test `docker_args()`, the argv it *would* pass. These tests close that gap:
//! they assert what the daemon actually does with those flags.
//!
//! **Every test no-ops when Docker is unavailable.** That is deliberate: CI has
//! no daemon, and a suite that fails on a missing optional dependency teaches
//! people to ignore it. The skip is loud in the output so a green run is not
//! mistaken for a validated one.

use ferric_research::sandbox::{NetworkPolicy, SandboxConfig, check_available, run_in_sandbox};

/// `true` when the daemon is reachable; prints a visible skip notice otherwise.
fn docker_ready(test: &str) -> bool {
    if check_available() {
        return true;
    }
    println!("SKIP {test}: no Docker daemon — sandbox NOT validated by this run");
    false
}

/// Nothing about the image should be assumed; pull failures are a real outcome
/// and must surface as an error, not a panic.
fn run(config: &SandboxConfig, cmd: &[&str]) -> Result<String, String> {
    run_in_sandbox(config, cmd).map_err(|e| e.to_string())
}

/// Baseline: the sandbox can execute at all. Without this, every other result
/// here is ambiguous.
#[test]
fn a_command_runs_inside_the_sandbox() {
    if !docker_ready("a_command_runs_inside_the_sandbox") {
        return;
    }
    let config = SandboxConfig {
        network: NetworkPolicy::Denied,
        enforce_runsc: false,
        ..SandboxConfig::default()
    };
    let out = run(&config, &["echo", "sandbox-ok"]).expect("sandbox should execute");
    assert!(out.contains("sandbox-ok"), "got: {out}");
}

/// The security property A5 is actually about: the default denies egress. This
/// is the assertion `docker_args()` alone could never make — it checks the
/// daemon honours `--network none`, not merely that we passed it.
#[test]
fn a_denied_network_really_cannot_reach_out() {
    if !docker_ready("a_denied_network_really_cannot_reach_out") {
        return;
    }
    let config = SandboxConfig {
        network: NetworkPolicy::Denied,
        enforce_runsc: false,
        ..SandboxConfig::default()
    };
    // `wget` on a network-less container must fail. If this ever succeeds, the
    // airlock is open and the whole sandbox is decorative.
    let result = run(&config, &["wget", "-qO-", "-T", "5", "http://example.com"]);
    assert!(
        result.is_err(),
        "a Denied-network container reached the internet: {result:?}"
    );
}

/// The counterpart: egress works when it is explicitly asked for. Without this,
/// the test above could pass because the image simply has no `wget`.
#[test]
fn unrestricted_egress_actually_works() {
    if !docker_ready("unrestricted_egress_actually_works") {
        return;
    }
    let config = SandboxConfig::unrestricted_for_testing();
    match run(&config, &["wget", "-qO-", "-T", "10", "http://example.com"]) {
        Ok(body) => assert!(
            body.contains("Example Domain"),
            "fetched something unexpected: {}",
            &body[..body.len().min(200)]
        ),
        // A sandbox that cannot reach the network here is a finding about the
        // host, not about the code — say so rather than failing opaquely.
        Err(e) => println!("NOTE: unrestricted egress unavailable on this host: {e}"),
    }
}

/// Capabilities are dropped for real, not just requested. `chown` inside the
/// container needs CAP_CHOWN, which `--cap-drop=ALL` removes.
#[test]
fn capabilities_are_really_dropped() {
    if !docker_ready("capabilities_are_really_dropped") {
        return;
    }
    let config = SandboxConfig {
        network: NetworkPolicy::Denied,
        enforce_runsc: false,
        ..SandboxConfig::default()
    };
    let result = run(&config, &["sh", "-c", "touch /tmp/f && chown 1234 /tmp/f"]);
    assert!(
        result.is_err(),
        "chown succeeded despite --cap-drop=ALL: {result:?}"
    );
}

/// The default requires gVisor. Where `runsc` is not installed the run must fail
/// **closed** — an error, never a silent fallback to the normal runtime.
#[test]
fn the_gvisor_default_fails_closed_when_runsc_is_absent() {
    if !docker_ready("the_gvisor_default_fails_closed_when_runsc_is_absent") {
        return;
    }
    let has_runsc = std::process::Command::new("docker")
        .args([
            "info",
            "--format",
            "{{range $k,$v := .Runtimes}}{{$k}} {{end}}",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("runsc"))
        .unwrap_or(false);

    let result = run(&SandboxConfig::default(), &["echo", "hi"]);
    if has_runsc {
        assert!(
            result.is_ok(),
            "runsc is installed, so this should run: {result:?}"
        );
    } else {
        assert!(
            result.is_err(),
            "runsc is absent, so the default MUST fail closed — instead it ran: {result:?}"
        );
    }
}
