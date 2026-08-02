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
        .args(["bench", "full", "--mock", "--level", "0"])
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
    assert_eq!(row["protocol"], "ConstrainedJson");
    // plan_steps is null (no planner — flagged, not faked).
    assert!(row["plan_steps"].is_null());
    // The row carries a terminator (the mock completes via task_complete).
    assert_eq!(row["terminator"], "task_complete");
}

#[test]
fn bench_mock_records_each_requested_level() {
    let results = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["bench", "full", "--mock", "--level", "3", "--level", "4"])
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
    let first: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
    assert_eq!(first["spec_version"], 2);
    assert_eq!(
        first["command_checks"][0]["status"], "model_failure",
        "a missing model artifact is grading evidence, not infrastructure failure"
    );
}

#[test]
fn bench_check_preflight_distinguishes_missing_python_infrastructure() {
    let results = tempfile::tempdir().unwrap();
    let missing_python = results.path().join("definitely-missing-python");
    let out = ferric()
        .args(["bench", "full", "--mock", "--level", "3"])
        .arg("--python-bin")
        .arg(&missing_python)
        .arg("--results-dir")
        .arg(results.path())
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("benchmark check infrastructure")
            && stderr.contains("cannot launch Python"),
        "missing interpreter must be surfaced as infrastructure: {stderr}"
    );
    assert!(
        !results.path().join("results.jsonl").exists(),
        "preflight must fail before recording a model result"
    );
}

#[test]
fn bench_keep_workspace_preserves_dir() {
    let results = tempfile::tempdir().unwrap();
    let out = ferric()
        .args([
            "bench",
            "full",
            "--mock",
            "--level",
            "0",
            "--keep-workspace",
        ])
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

#[test]
fn bench_three_trials_rotate_levels_retain_traces_and_write_statistics() {
    let results = tempfile::tempdir().unwrap();
    let out = ferric()
        .args([
            "bench",
            "full",
            "--mock",
            "--trials",
            "3",
            "--min-pass-rate",
            "0.67",
            "--level",
            "0",
            "--level",
            "1",
            "--level",
            "2",
        ])
        .arg("--results-dir")
        .arg(results.path())
        .output()
        .unwrap();
    let _ = out.status;

    let rows: Vec<serde_json::Value> =
        std::fs::read_to_string(results.path().join("results.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(rows.len(), 9);
    let observed: Vec<(String, u64)> = rows
        .iter()
        .map(|row| {
            (
                row["trial_id"].as_str().unwrap().to_string(),
                row["level"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        observed,
        vec![
            ("trial-001".to_string(), 0),
            ("trial-001".to_string(), 1),
            ("trial-001".to_string(), 2),
            ("trial-002".to_string(), 1),
            ("trial-002".to_string(), 2),
            ("trial-002".to_string(), 0),
            ("trial-003".to_string(), 2),
            ("trial-003".to_string(), 0),
            ("trial-003".to_string(), 1),
        ],
        "each trial rotates its starting rung"
    );
    let run_id = rows[0]["run_id"].as_str().unwrap();
    assert!(rows.iter().all(|row| row["run_id"] == run_id));
    assert!(rows.iter().all(|row| {
        row["started_at_unix_ms"].as_u64().is_some()
            && row["finished_at_unix_ms"].as_u64().is_some()
            && results
                .path()
                .join(row["trace_path"].as_str().unwrap())
                .is_file()
    }));

    let summary_path = results.path().join(format!("summary-{run_id}.json"));
    let summary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(summary_path).unwrap()).unwrap();
    assert_eq!(summary["trials_requested"], 3);
    assert_eq!(summary["trials_completed"], 3);
    assert_eq!(summary["observed_rows"], 9);
    assert!(
        summary["finished_at_unix_ms"].as_u64().unwrap()
            >= summary["started_at_unix_ms"].as_u64().unwrap()
    );
    assert_eq!(summary["provenance"]["model"]["backend"], "mock");
    assert!(summary["provenance"]["binary"]["path"].is_string());
    assert_eq!(summary["provenance"]["protocol"], "ConstrainedJson");
    assert_eq!(summary["levels"][0]["required_passes"], 3);
    assert!(summary["levels"][0]["wilson_95"]["lower"].is_number());
    assert!(summary["levels"][0]["wall_ms"]["median"].is_number());
    assert_eq!(summary["levels"][0]["terminal_counts"]["task_complete"], 3);
    assert_eq!(summary["calibration"]["eligible"], false);
    assert_eq!(
        summary["calibration"]["ineligible_reason"],
        "partial ladder"
    );
}

#[test]
fn bench_rejects_out_of_range_trial_and_pass_rate_arguments() {
    for args in [
        ["--trials", "0"],
        ["--trials", "101"],
        ["--min-pass-rate", "0"],
        ["--min-pass-rate", "1.01"],
    ] {
        let out = ferric()
            .args(["bench", "full", "--mock", "--level", "0"])
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success(), "invalid args unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("invalid value"),
            "clap should explain the rejected value: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
