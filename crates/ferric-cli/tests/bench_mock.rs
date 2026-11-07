//! T-215 integration: `ferric bench --mock` — the model-free, CI-runnable
//! self-test of the whole harness (spawn-self runner + verification +
//! results.jsonl + calibration), driven through the real binary.

use std::process::{Command, Output};

#[path = "../src/test_process_containment.rs"]
mod test_process_containment;

fn ferric() -> Command {
    test_process_containment::ensure_current_process_tree_is_contained()
        .expect("install benchmark integration-test process containment");
    Command::new(env!("CARGO_BIN_EXE_ferric"))
}

fn output(command: &mut Command) -> Output {
    test_process_containment::output_bounded(command, std::time::Duration::from_secs(60)).unwrap()
}

#[test]
fn bench_mock_v2_checks_record_model_failure_with_cargo_fixture() {
    let results = tempfile::tempdir().unwrap();

    // The benchmark preflight only requires its configured Python command to
    // accept `--version`. The Cargo-built Ferric fixture does that, while its
    // deliberate rejection of Python's `-m` arguments gives the v2 grader a
    // deterministic non-zero check result. This keeps the integration test
    // source-driven and independent of any machine Python alias.
    let out = output(
        ferric()
            .args(["bench", "full", "--mock", "--level", "3", "--level", "4"])
            .arg("--python-bin")
            .arg(env!("CARGO_BIN_EXE_ferric"))
            .arg("--results-dir")
            .arg(results.path()),
    );
    let _ = out.status;

    let content =
        std::fs::read_to_string(results.path().join("results.jsonl")).unwrap_or_else(|error| {
            panic!(
                "v2 mock run did not write results: {error}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        });
    let rows: Vec<serde_json::Value> = content
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid benchmark result row"))
        .collect();

    assert_eq!(rows.len(), 2, "one result row per requested v2 level");
    assert_eq!(rows[0]["level"], 3);
    assert_eq!(rows[1]["level"], 4);
    for row in rows {
        assert_eq!(row["spec_version"], 2);
        assert_eq!(
            row["command_checks"][0]["status"], "model_failure",
            "a non-zero trusted check is grading evidence, not infrastructure failure"
        );
    }
}

#[test]
fn bench_check_preflight_distinguishes_missing_python_infrastructure() {
    let results = tempfile::tempdir().unwrap();
    let missing_python = results.path().join("definitely-missing-python");
    let out = output(
        ferric()
            .args(["bench", "full", "--mock", "--level", "3"])
            .arg("--python-bin")
            .arg(&missing_python)
            .arg("--results-dir")
            .arg(results.path()),
    );

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
    let out = output(
        ferric()
            .args([
                "bench",
                "full",
                "--mock",
                "--level",
                "0",
                "--keep-workspace",
            ])
            .arg("--results-dir")
            .arg(results.path()),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("workspace kept:"),
        "kept-workspace path must be reported: {stdout}"
    );
}

#[test]
fn bench_three_trials_rotate_levels_retain_traces_and_write_statistics() {
    let results = tempfile::tempdir().unwrap();
    let out = output(
        ferric()
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
            .arg(results.path()),
    );
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
    // Every requested rung records one attributable row; the mock completes
    // via task_complete, and plan_steps is null (no planner — flagged, not faked).
    assert!(rows.iter().all(|row| {
        row["protocol"] == "ConstrainedJson"
            && row["terminator"] == "task_complete"
            && row["plan_steps"].is_null()
    }));
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
        let out = output(
            ferric()
                .args(["bench", "full", "--mock", "--level", "0"])
                .args(args),
        );
        assert!(!out.status.success(), "invalid args unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("invalid value"),
            "clap should explain the rejected value: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
