//! Calibration (T-214, ADR-019): turn a sweep's results into a model's
//! `measured_level`, the input that overrides parameter-count tiering in
//! `ferric_core::policy_for`. `ferric bench` is the SOLE producer of this
//! value — tier assignments change only with a committed measurement diff.

use std::path::Path;

use ferric_core::{tier_for_level, tier_for_params};
use serde::{Deserialize, Serialize};

use crate::results::ResultRow;

/// A model's calibration record, keyed by model file in `model_profiles.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProfileRecord {
    pub model: String,
    pub params_b: f32,
    pub protocol: String,
    /// Highest level the model completed (None = failed L0).
    pub measured_level: Option<u8>,
    pub tier_from_params: String,
    pub tier_from_measured: Option<String>,
}

/// The highest level with `completed == true` across a model's rows.
pub fn highest_completed_level(rows: &[ResultRow]) -> Option<u8> {
    rows.iter().filter(|r| r.completed).map(|r| r.level).max()
}

/// Build a calibration record from a sweep's rows for one model.
pub fn calibrate(
    model: &str,
    params_b: f32,
    protocol: &str,
    rows: &[ResultRow],
) -> ModelProfileRecord {
    let measured_level = highest_completed_level(rows);
    ModelProfileRecord {
        model: model.to_string(),
        params_b,
        protocol: protocol.to_string(),
        measured_level,
        tier_from_params: format!("{:?}", tier_for_params(params_b)),
        tier_from_measured: measured_level.map(|l| format!("{:?}", tier_for_level(l))),
    }
}

/// Write/replace this model+protocol's record in `<dir>/model_profiles.json`
/// (a JSON array; existing records for other model/protocol keys are kept).
pub fn write_profile(dir: &Path, record: &ModelProfileRecord) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("model_profiles.json");
    let mut records: Vec<ModelProfileRecord> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    records.retain(|r| !(r.model == record.model && r.protocol == record.protocol));
    records.push(record.clone());
    let text = serde_json::to_string_pretty(&records).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(level: u8, completed: bool) -> ResultRow {
        ResultRow {
            level,
            level_name: format!("l{level}"),
            variant: "v".to_string(),
            protocol: "unified_grammar".to_string(),
            model: Some("llama-1b".to_string()),
            completed,
            timed_out: false,
            exit_code: Some(0),
            turns: 1,
            input_tokens: 0,
            output_tokens: 0,
            wall_ms: 0,
            terminator: Some("task_complete".to_string()),
            tier_observed: None,
            protocol_observed: None,
            repetition_guard_fires: 0,
            tools_called: vec![],
            task_complete_summary: None,
            failure_admission: None,
            plan_steps: None,
            expectations_ok: true,
            tools_ok: true,
            tier_from_params: "Nano".to_string(),
            stderr_tail: String::new(),
        }
    }

    #[test]
    fn highest_completed_is_measured_level() {
        let rows = vec![row(0, true), row(1, true), row(2, false), row(3, false)];
        assert_eq!(highest_completed_level(&rows), Some(1));
    }

    #[test]
    fn calibrate_records_both_tiers() {
        // A 1B that completes L4 → measured_level 4 → tier_for_level(4)=Small,
        // even though params (1.0) → Nano. This is the override in action.
        let rows = vec![row(0, true), row(2, true), row(4, true)];
        let rec = calibrate("llama-1b", 1.0, "unified_grammar", &rows);
        assert_eq!(rec.measured_level, Some(4));
        assert_eq!(rec.tier_from_params, "Nano");
        assert_eq!(rec.tier_from_measured.as_deref(), Some("Small"));
    }

    #[test]
    fn write_profile_replaces_same_key_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(
            dir.path(),
            &calibrate("llama-1b", 1.0, "unified_grammar", &[row(0, true)]),
        )
        .unwrap();
        write_profile(
            dir.path(),
            &calibrate(
                "qwen-7b",
                7.0,
                "unified_grammar",
                &[row(0, true), row(3, true)],
            ),
        )
        .unwrap();
        // Re-calibrate llama under the same protocol → replaces, not appends.
        write_profile(
            dir.path(),
            &calibrate(
                "llama-1b",
                1.0,
                "unified_grammar",
                &[row(0, true), row(1, true)],
            ),
        )
        .unwrap();

        let text = std::fs::read_to_string(dir.path().join("model_profiles.json")).unwrap();
        let recs: Vec<ModelProfileRecord> = serde_json::from_str(&text).unwrap();
        assert_eq!(recs.len(), 2, "two distinct model keys");
        let llama = recs.iter().find(|r| r.model == "llama-1b").unwrap();
        assert_eq!(
            llama.measured_level,
            Some(1),
            "replaced with new measurement"
        );
    }

    #[test]
    fn failed_l0_has_no_measured_level() {
        let rec = calibrate("weak", 1.0, "native_tools", &[row(0, false)]);
        assert_eq!(rec.measured_level, None);
        assert_eq!(rec.tier_from_measured, None);
    }
}
