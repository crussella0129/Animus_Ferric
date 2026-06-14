//! T-215 integration: `ferric bench --mock` — the model-free, CI-runnable
//! self-test of the whole harness (spawn-self runner + verification +
//! results.jsonl + calibration), driven through the real binary.

use std::process::Command;

fn ferric() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferric"))
}

#[test]
fn bench_mock_l0_passes_and_writes_results() {
    let results = tempfile::tempdir().unwrap();
    // L0 with the built-in mock: the mock writes ferric-mock.txt and calls
    // task_complete. L0's spec expects list_dir (any_of) and forbids writes,
    // so the mock will FAIL L0 — which is itself a valid, recorded outcome.
    // Use L3 instead: it expects write_file + task_complete, which the mock
    // does. But the mock writes "ferric-mock.txt", not greet.py, so L3's file
    // expectation fails too. The honest assertion here is operational: the
    // harness runs, verifies, and records a row — not that the mock passes a
    // real task. Assert the machinery, not a fake pass.
    let out = ferric()
        .args(["bench", "--mock", "--level", "0"])
        .arg("--results-dir")
        .arg(results.path())
        .output()
        .unwrap();
    // The harness ran to completion (exit reflects pass/fail of the level;
    // either is fine — we assert the row was written).
    let _ = out.status;

    let results_file = results.path().join("results.jsonl");
    assert!(results_file.exists(), "results.jsonl must be written");
    let content = std::fs::read_to_string(&results_file).unwrap();
    let row: serde_json::Value =
        serde_json::from_str(content.lines().next().expect("one row")).unwrap();
    assert_eq!(row["level"], 0);
    assert_eq!(row["protocol"], "UnifiedGrammar");
    // plan_steps is null (no planner — flagged, not faked).
    assert!(row["plan_steps"].is_null());
    // The row carries a terminator (the mock completes via task_complete).
    assert_eq!(row["terminator"], "task_complete");
}

#[test]
fn bench_mock_records_each_requested_level() {
    let results = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["bench", "--mock", "--level", "3", "--level", "4"])
        .arg("--results-dir")
        .arg(results.path())
        .output()
        .unwrap();
    let _ = out.status;
    let content = std::fs::read_to_string(results.path().join("results.jsonl")).unwrap();
    let levels: Vec<u64> = content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["level"].as_u64())
        .collect();
    assert_eq!(levels, vec![3, 4], "one row per requested level, in order");
}

#[test]
fn bench_keep_workspace_preserves_dir() {
    let results = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["bench", "--mock", "--level", "0", "--keep-workspace"])
        .arg("--results-dir")
        .arg(results.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("workspace kept:"),
        "kept-workspace path must be reported: {stdout}"
    );
}
