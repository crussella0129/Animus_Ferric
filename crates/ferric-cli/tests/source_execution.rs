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

    // Backend-free CI cannot accidentally include the positive startup/human
    // fixtures that require native listener visibility on Linux.
    // The command surface (including the feature-gated module declarations) now
    // lives in the library; the binaries are thin shims over it.
    let lib = include_str!("../src/lib.rs").replace("\r\n", "\n");
    assert!(lib.contains("#[cfg(feature = \"backend-openai\")]\nmod startup;"));
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

    // The default HTTP backend includes ring's C build. Cross checks need the
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
    assert!(
        !cross_gate.contains("--no-default-features"),
        "the real default backend must remain covered by portability checks"
    );
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
