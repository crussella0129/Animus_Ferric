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
        "cargo clippy -p ferric-cli --no-default-features --all-targets --locked -- -D warnings",
        "cargo test -p ferric-cli --no-default-features --locked",
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
        .split("\n  no-default-features:\n")
        .next()
        .unwrap();
    assert!(
        workspace_gate
            .contains("if: runner.os == 'Windows'\n        run: cargo test --workspace --locked")
    );
    assert!(workspace_gate.contains(
        "if: runner.os == 'Linux'\n        shell: bash\n        run: bash tools/test-lifecycle-linux.sh workspace"
    ));

    // Backend-free CI cannot accidentally include the positive startup/human
    // fixtures that require native listener visibility on Linux.
    let main = include_str!("../src/main.rs").replace("\r\n", "\n");
    assert!(main.contains("#[cfg(feature = \"backend-openai\")]\nmod startup;"));
    let human = include_str!("../src/human.rs").replace("\r\n", "\n");
    assert!(human.contains("#[cfg(feature = \"backend-openai\")]\nmod enabled {"));
    let backend_free_gate = workflow
        .split_once("\n  no-default-features:\n")
        .expect("backend-free job")
        .1
        .split("\n  backend-check:\n")
        .next()
        .unwrap();
    for line in backend_free_gate
        .lines()
        .filter(|line| line.contains("run:"))
    {
        assert!(line.contains("--no-default-features"), "{line}");
        assert!(!line.contains("--features"), "{line}");
        assert!(!line.contains("test-lifecycle-linux.sh"), "{line}");
    }
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
