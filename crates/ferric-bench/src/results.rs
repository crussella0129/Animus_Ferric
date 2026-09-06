//! The append-only results row (T-213) and the JSONL ledger.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::verify::CommandCheckResult;

/// One benchmark run's recorded outcome. Append-only to `results.jsonl`;
/// `null` fields are metrics Ferric cannot yet source (flagged, not faked).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRow {
    /// Attribution fields added by summary schema v1. Legacy rows deserialize
    /// with `None` and remain readable by calibration/inspection tooling.
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub trial_id: Option<String>,
    #[serde(default)]
    pub started_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub finished_at_unix_ms: Option<u64>,
    /// Results-dir-relative path to the retained JSONL trace.
    #[serde(default)]
    pub trace_path: Option<String>,
    /// Parent enforcement and separately observed child output provenance.
    /// Legacy rows have unknown attribution, not an assumed default budget.
    #[serde(default)]
    pub budget: Option<crate::budget::AttemptBudgetEvidence>,
    /// Harness/infrastructure failure attributable to this attempt, if any.
    #[serde(default)]
    pub infrastructure_error: Option<String>,
    pub level: u8,
    #[serde(default = "crate::spec::default_spec_version")]
    pub spec_version: u32,
    pub level_name: String,
    pub variant: String,
    pub protocol: String,
    pub model: Option<String>,
    pub completed: bool,
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub wall_ms: u64,
    pub terminator: Option<String>,
    pub tier_observed: Option<String>,
    pub protocol_observed: Option<String>,
    pub repetition_guard_fires: u32,
    pub tools_called: Vec<String>,
    pub task_complete_summary: Option<String>,
    pub failure_admission: Option<String>,
    /// No planner yet (ADR-019): always null this sprint.
    pub plan_steps: Option<u32>,
    pub expectations_ok: bool,
    pub tools_ok: bool,
    /// Detailed authoritative post-run grading. Empty for legacy rows/specs.
    #[serde(default)]
    pub command_checks: Vec<CommandCheckResult>,
    /// Tier the param-count would assign vs the measured/observed tier.
    pub tier_from_params: String,
    pub stderr_tail: String,
}

/// Append one row as a JSON line to `<dir>/results.jsonl` (creating the dir).
pub fn append_row(dir: &Path, row: &ResultRow) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("results.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(row).map_err(std::io::Error::other)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Read all rows back (for calibration / inspection).
pub fn read_rows(dir: &Path) -> std::io::Result<Vec<ResultRow>> {
    let path = dir.join("results.jsonl");
    let text = std::fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row = serde_json::from_str(line).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid results.jsonl line {}: {error}", index + 1),
            )
        })?;
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(level: u8, completed: bool) -> ResultRow {
        ResultRow {
            run_id: Some("run-test".to_string()),
            trial_id: Some("trial-001".to_string()),
            started_at_unix_ms: Some(1),
            finished_at_unix_ms: Some(2),
            trace_path: Some("traces/run-test/trial-001-l0.jsonl".to_string()),
            budget: None,
            infrastructure_error: None,
            level,
            spec_version: 1,
            level_name: format!("l{level}"),
            variant: "test".to_string(),
            protocol: "unified_grammar".to_string(),
            model: Some("llama-1b".to_string()),
            completed,
            timed_out: false,
            exit_code: Some(0),
            turns: 3,
            input_tokens: 100,
            output_tokens: 50,
            wall_ms: 1200,
            terminator: Some("task_complete".to_string()),
            tier_observed: Some("Nano".to_string()),
            protocol_observed: Some("ConstrainedJson".to_string()),
            repetition_guard_fires: 0,
            tools_called: vec!["write_file".to_string(), "task_complete".to_string()],
            task_complete_summary: Some("done".to_string()),
            failure_admission: None,
            plan_steps: None,
            expectations_ok: true,
            tools_ok: true,
            command_checks: Vec::new(),
            tier_from_params: "Nano".to_string(),
            stderr_tail: String::new(),
        }
    }

    #[test]
    fn append_is_not_truncate() {
        let dir = tempfile::tempdir().unwrap();
        append_row(dir.path(), &row(0, true)).unwrap();
        append_row(dir.path(), &row(1, false)).unwrap();
        let rows = read_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 2, "second append must not truncate the first");
        assert_eq!(rows[0].level, 0);
        assert_eq!(rows[1].level, 1);
        assert!(
            rows[0].plan_steps.is_none(),
            "plan_steps is null, not faked"
        );
    }

    #[test]
    fn legacy_row_without_spec_or_checks_uses_compatible_defaults() {
        let mut value = serde_json::to_value(row(0, true)).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("spec_version");
        object.remove("command_checks");
        object.remove("run_id");
        object.remove("trial_id");
        object.remove("started_at_unix_ms");
        object.remove("finished_at_unix_ms");
        object.remove("trace_path");
        object.remove("infrastructure_error");
        object.remove("budget");

        let legacy: ResultRow = serde_json::from_value(value).unwrap();
        assert_eq!(legacy.spec_version, 1);
        assert!(legacy.command_checks.is_empty());
        assert!(legacy.run_id.is_none());
        assert!(legacy.trial_id.is_none());
        assert!(legacy.budget.is_none());
    }

    #[test]
    fn read_rows_reports_the_first_bad_physical_line() {
        let dir = tempfile::tempdir().unwrap();
        let valid = serde_json::to_string(&row(0, true)).unwrap();
        std::fs::write(
            dir.path().join("results.jsonl"),
            format!("{valid}\n\n{{not-json}}\n{valid}\n"),
        )
        .unwrap();

        let error = read_rows(dir.path()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("line 3"),
            "bad physical line must be attributable: {error}"
        );
    }
}
