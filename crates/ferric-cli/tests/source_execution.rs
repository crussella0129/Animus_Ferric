//! A static ratchet complements the real Linux CI execution. It must not become
//! a substitute for native success and source-owned cleanup evidence.

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
    assert!(launch.contains("--pid --net --fork --mount-proc --kill-child=SIGKILL"));
    assert!(launch.contains("setpriv --pdeathsig keep"));
    assert!(
        launch.contains("--no-new-privs --inh-caps=-all --ambient-caps=-all --bounding-set=-all")
    );
    assert!(reaper.contains("test \"$$\" -eq 1"));
    assert!(reaper.contains("test \"$(id -u)\" -ne 0"));
    assert!(reaper.contains("\"$ferric_cargo\" test -p ferric-cli"));
    assert!(reaper.contains("--locked --offline -- --test-threads=1"));
    assert!(!reaper.contains("exec \"$ferric_cargo\""));
    assert!(reaper.contains("|| ferric_status=$?"));
    assert!(reaper.contains("exit \"$ferric_status\""));
}
