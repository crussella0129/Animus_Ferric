//! The append-only results row (T-213) and the JSONL ledger.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One benchmark run's recorded outcome. Append-only to `results.jsonl`;
/// `null` fields are metrics Ferric cannot yet source (flagged, not faked).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRow {
    pub level: u8,
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
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(level: u8, completed: bool) -> ResultRow {
        ResultRow {
            level,
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
}
