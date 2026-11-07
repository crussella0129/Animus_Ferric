//! A static ratchet complements the real Linux CI execution. It must not become
//! a substitute for native success and source-owned cleanup evidence.

#[test]
fn source_quality_and_feature_matrix() {
    let workflow = include_str!("../../../.github/workflows/ci.yml").replace("\r\n", "\n");
    // A CLI default-member must not silently narrow workspace quality gates.
    for required in [
        "cargo fmt --all --check",
        "rustfmt --edition 2024 --check crates/ferric-cli/src/human_journey_tests.rs",
        "cargo clippy --workspace --all-targets --locked -- -D warnings",
        "cargo test --workspace --locked",
        "bash tools/test-lifecycle-linux.sh workspace",
    ] {
        assert!(
            workflow.contains(required),
            "missing source quality gate: {required}"
        );
    }
    let workspace_gate = workflow
        .split_once("\n  test:\n")
        .expect("workspace job")
        .1
        .split("\n  lifecycle-fixture:\n")
        .next()
        .unwrap();
    let windows_gate = workspace_gate
        .split_once("- name: workspace tests (Windows)\n")
        .expect("native Windows workspace gate")
        .1
        .split("- name: workspace tests (isolated non-root Linux)")
        .next()
        .unwrap();
    assert!(windows_gate.contains("if: runner.os == 'Windows'\n"));
    let windows_command = "cargo test --workspace --locked -- --test-threads=1";
    assert!(
        windows_gate
            .lines()
            .any(|line| line.trim() == format!("run: {windows_command}")),
        "keep the full native suite, finite fixture budgets and isolated inter-test schedule"
    );
    assert!(include_str!("../../../docs/process-execution.md").contains(windows_command));
    assert!(workspace_gate.contains(
        "if: runner.os == 'Linux'\n        shell: bash\n        run: bash tools/test-lifecycle-linux.sh workspace"
    ));

    // The HTTP backend is always compiled (INT-0018): there is no backend-free
    // lane, so the startup and positive human-session fixtures always run, on
    // Linux inside the namespace workspace gate above that gives them native
    // listener visibility. No feature can drop them from the library (the
    // binaries are thin shims over it).
    assert!(
        !workflow.contains("--no-default-features"),
        "the backend-free CLI lane was retired"
    );
    for (name, source) in [
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/human.rs", include_str!("../src/human.rs")),
    ] {
        assert!(
            !source.contains("feature = \""),
            "{name}: a feature gates the command surface"
        );
    }

    // The HTTP backend includes ring's C build. Cross checks need the
    // target C compiler and libc headers even though cargo check does not link
    // Ferric itself. Install headers explicitly, not through apt recommendations.
    let cross_gate = workflow
        .split_once("\n  aarch64-check:\n")
        .expect("aarch64 portability job")
        .1;
    let install = "sudo apt-get install --yes --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross";
    for required in [
        "runs-on: ubuntu-latest",
        "CC_aarch64_unknown_linux_gnu: aarch64-linux-gnu-gcc",
        "sudo apt-get update",
        install,
        "cargo check --workspace --target aarch64-unknown-linux-gnu --locked",
        "cargo check -p ferric-cli --features lifecycle-fixture --all-targets --target aarch64-unknown-linux-gnu --locked",
    ] {
        assert!(
            cross_gate.contains(required),
            "missing cross gate: {required}"
        );
    }
    assert!(cross_gate.find(install).unwrap() < cross_gate.find("run: cargo check").unwrap());
}

#[test]
fn source_driven_ci_contract() {
    let workflow = include_str!("../../../.github/workflows/ci.yml");
    let launch = include_str!("../../../tools/test-lifecycle-linux.sh");
    let reaper = include_str!("../../../tools/lifecycle-linux-reaper.sh");

    assert!(workflow.contains("run: bash tools/test-lifecycle-linux.sh"));
    for source in [workflow, launch, reaper] {
        for retired in [
            ".executable //",
            "--message-format=json",
            "test_bin=",
            "target/debug/",
        ] {
            assert!(
                !source.contains(retired),
                "direct-artifact pattern: {retired}"
            );
        }
    }
    assert!(launch.contains("cargo test -p ferric-cli --features lifecycle-fixture"));
    assert!(launch.contains("--test server_lifecycle_fixture --locked --no-run"));
    assert!(launch.contains("cargo test --workspace --locked --no-run"));
    for source in [launch, reaper] {
        assert!(source.contains("case \"$ferric_mode\" in"));
        assert!(source.contains("lifecycle|workspace)"));
    }
    assert!(launch.contains("\"$PATH\" \"$ferric_mode\""));
    assert!(reaper.contains("ferric_mode=$7"));
    assert!(launch.contains("--pid --net --fork --mount-proc --kill-child=SIGKILL"));
    assert!(launch.contains("setpriv --pdeathsig keep"));
    assert!(
        launch.contains("--no-new-privs --inh-caps=-all --ambient-caps=-all --bounding-set=-all")
    );
    assert!(reaper.contains("test \"$$\" -eq 1"));
    assert!(reaper.contains("test \"$(id -u)\" -ne 0"));
    assert!(reaper.contains("\"$ferric_cargo\" test -p ferric-cli"));
    assert!(
        reaper
            .contains("\"$ferric_cargo\" test --workspace --locked --offline -- --test-threads=1")
    );
    assert!(reaper.contains("--locked --offline -- --test-threads=1"));
    assert!(!reaper.contains("exec \"$ferric_cargo\""));
    assert!(reaper.contains("|| ferric_status=$?"));
    assert!(reaper.contains("exit \"$ferric_status\""));
}
