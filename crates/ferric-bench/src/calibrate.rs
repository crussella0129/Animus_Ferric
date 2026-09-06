//! Calibration (T-214, ADR-019): turn a sweep's results into a model's
//! `measured_level`, the input that overrides parameter-count tiering in
//! `ferric_core::policy_for`. `ferric bench` is the SOLE producer of this
//! value — tier assignments change only with a committed measurement diff.

use std::path::Path;

use ferric_core::{tier_for_level, tier_for_params};
use serde::{Deserialize, Serialize};

use crate::results::ResultRow;
use crate::summary::{CalibrationEvidence, required_passes};

/// A model's calibration record, keyed by model file in `model_profiles.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProfileRecord {
    pub model: String,
    pub params_b: f32,
    pub protocol: String,
    /// Highest rung in the model's qualified contiguous L0 prefix.
    pub measured_level: Option<u8>,
    pub tier_from_params: String,
    pub tier_from_measured: Option<String>,
    /// Highest tool ring the model drove `solid` in a `--calibrate-rings` sweep
    /// (ADR-028/029) — the recommended `--max-ring`. Additive: records written
    /// before this field deserialize with `None`.
    #[serde(default)]
    pub calibrated_ring: Option<u8>,
    /// Evidence for the repeated-trial calibration that produced this level.
    /// Optional so profiles written before summary schema v1 remain valid.
    #[serde(default)]
    pub calibration_evidence: Option<CalibrationEvidence>,
}

/// The highest level with `completed == true` across a model's rows.
pub fn highest_completed_level(rows: &[ResultRow]) -> Option<u8> {
    rows.iter().filter(|r| r.completed).map(|r| r.level).max()
}

/// Longest completed contiguous prefix beginning at L0.
///
/// A pass above a failed or absent lower rung cannot raise capability. This is
/// intentionally stricter than the historical `max(passed)` calibration.
pub fn longest_completed_prefix(rows: &[ResultRow]) -> Option<u8> {
    let mut level = 0_u8;
    while rows.iter().any(|row| row.level == level && row.completed) {
        if level == u8::MAX {
            return Some(level);
        }
        level += 1;
    }
    level.checked_sub(1)
}

/// Levels that FAILED while some higher level passed — i.e. the ladder did not
/// behave monotonically.
///
/// This remains useful diagnostic evidence even though calibration now stops
/// at the first unqualified rung instead of flattening the ladder to max(pass).
pub fn non_monotonic_failures(rows: &[ResultRow]) -> Vec<u8> {
    let Some(top) = highest_completed_level(rows) else {
        return Vec::new();
    };
    let mut out: Vec<u8> = rows
        .iter()
        .filter(|r| !r.completed && r.level < top)
        .map(|r| r.level)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Build a calibration record from a sweep's rows for one model.
pub fn calibrate(
    model: &str,
    params_b: f32,
    protocol: &str,
    rows: &[ResultRow],
) -> ModelProfileRecord {
    let diagnostic = rows
        .iter()
        .filter_map(|row| row.budget.as_ref())
        .any(|budget| budget.controls.is_diagnostic());
    let measured_level = (!diagnostic)
        .then(|| longest_completed_prefix(rows))
        .flatten();
    ModelProfileRecord {
        model: model.to_string(),
        params_b,
        protocol: protocol.to_string(),
        measured_level,
        tier_from_params: format!("{:?}", tier_for_params(params_b)),
        tier_from_measured: measured_level.map(|l| format!("{:?}", tier_for_level(l))),
        calibrated_ring: None,
        calibration_evidence: None,
    }
}

/// Build a profile only when a complete, infrastructure-clean full ladder
/// produced eligible repeated-trial calibration evidence.
pub fn calibrate_from_evidence(
    model: &str,
    params_b: f32,
    protocol: &str,
    evidence: &CalibrationEvidence,
) -> Option<ModelProfileRecord> {
    let derived_level = longest_level_prefix(&evidence.qualified_levels);
    let unique_levels: std::collections::BTreeSet<u8> =
        evidence.qualified_levels.iter().copied().collect();
    let numeric_evidence_valid = (1..=100).contains(&evidence.trials)
        && evidence.min_pass_rate.is_finite()
        && evidence.min_pass_rate > 0.0
        && evidence.min_pass_rate <= 1.0
        && evidence.required_passes == required_passes(evidence.trials, evidence.min_pass_rate)
        && unique_levels.len() == evidence.qualified_levels.len()
        && unique_levels.iter().all(|level| *level <= 6);
    let attribution_valid = !evidence.run_id.is_empty()
        && evidence.summary_file == format!("summary-{}.json", evidence.run_id);
    if !evidence.eligible
        || evidence.is_diagnostic()
        || !evidence.full_ladder
        || !evidence.complete
        || !evidence.infrastructure_clean
        || evidence.measured_level != derived_level
        || !numeric_evidence_valid
        || !attribution_valid
    {
        return None;
    }
    Some(ModelProfileRecord {
        model: model.to_string(),
        params_b,
        protocol: protocol.to_string(),
        measured_level: derived_level,
        tier_from_params: format!("{:?}", tier_for_params(params_b)),
        tier_from_measured: derived_level.map(|level| format!("{:?}", tier_for_level(level))),
        calibrated_ring: None,
        calibration_evidence: Some(evidence.clone()),
    })
}

fn longest_level_prefix(levels: &[u8]) -> Option<u8> {
    let levels: std::collections::BTreeSet<u8> = levels.iter().copied().collect();
    let mut level = 0_u8;
    while levels.contains(&level) {
        if level == u8::MAX {
            return Some(level);
        }
        level += 1;
    }
    level.checked_sub(1)
}

/// Read this model+protocol's record from `<dir>/model_profiles.json`, or `None`
/// when the file or the (model, protocol) key is absent. The read-back consumer
/// (`ferric query`) applies `measured_level` (→ tier) and `calibrated_ring`
/// (→ `max_ring`); a miss is a safe no-op (ADR-029).
pub fn read_profile(dir: &Path, model: &str, protocol: &str) -> Option<ModelProfileRecord> {
    let path = dir.join("model_profiles.json");
    let records: Vec<ModelProfileRecord> =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    records
        .into_iter()
        .find(|r| r.model == model && r.protocol == protocol)
}

/// Persist a `--calibrate-rings` result: load-or-create the (model, protocol)
/// record and set ONLY `calibrated_ring`, preserving any `measured_level` the
/// L0–L6 bench wrote. Uses the same merge-by-key discipline as `write_profile`.
pub fn write_calibrated_ring(
    dir: &Path,
    model: &str,
    protocol: &str,
    params_b: f32,
    ring: u8,
) -> std::io::Result<()> {
    let mut record = read_profile(dir, model, protocol).unwrap_or_else(|| ModelProfileRecord {
        model: model.to_string(),
        params_b,
        protocol: protocol.to_string(),
        measured_level: None,
        tier_from_params: format!("{:?}", tier_for_params(params_b)),
        tier_from_measured: None,
        calibrated_ring: None,
        calibration_evidence: None,
    });
    record.calibrated_ring = Some(ring);
    write_profile(dir, &record)
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
            run_id: None,
            trial_id: None,
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
            trace_path: None,
            budget: None,
            infrastructure_error: None,
            level,
            spec_version: 1,
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
            command_checks: Vec::new(),
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
        let rows = (0..=4).map(|level| row(level, true)).collect::<Vec<_>>();
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

    #[test]
    fn read_profile_round_trips_and_misses_are_none() {
        let dir = tempfile::tempdir().unwrap();
        // Missing file → None.
        assert_eq!(read_profile(dir.path(), "m", "ConstrainedJson"), None);
        let rec = calibrate(
            "qwen-7b",
            7.0,
            "ConstrainedJson",
            &[row(0, true), row(3, true)],
        );
        write_profile(dir.path(), &rec).unwrap();
        // Exact key round-trips; a wrong key → None.
        assert_eq!(
            read_profile(dir.path(), "qwen-7b", "ConstrainedJson").as_ref(),
            Some(&rec)
        );
        assert_eq!(read_profile(dir.path(), "qwen-7b", "NativeTools"), None);
        assert_eq!(read_profile(dir.path(), "absent", "ConstrainedJson"), None);
    }

    #[test]
    fn write_calibrated_ring_preserves_measured_level() {
        let dir = tempfile::tempdir().unwrap();
        // A model the L0–L6 bench measured to level 4.
        let rec = calibrate(
            "llama-1b",
            1.0,
            "ConstrainedJson",
            &(0..=4).map(|level| row(level, true)).collect::<Vec<_>>(),
        );
        assert_eq!(rec.measured_level, Some(4));
        write_profile(dir.path(), &rec).unwrap();
        // Now the ring calibration writes ring 1 — it must NOT clobber the level.
        write_calibrated_ring(dir.path(), "llama-1b", "ConstrainedJson", 1.0, 1).unwrap();
        let back = read_profile(dir.path(), "llama-1b", "ConstrainedJson").unwrap();
        assert_eq!(back.measured_level, Some(4), "level preserved");
        assert_eq!(back.calibrated_ring, Some(1), "ring set");
    }

    #[test]
    fn write_calibrated_ring_creates_record_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        write_calibrated_ring(dir.path(), "fresh", "ConstrainedJson", 7.0, 2).unwrap();
        let back = read_profile(dir.path(), "fresh", "ConstrainedJson").unwrap();
        assert_eq!(back.calibrated_ring, Some(2));
        assert_eq!(back.measured_level, None);
    }

    #[test]
    fn old_json_without_ring_field_defaults_none() {
        // A record serialized before `calibrated_ring` existed.
        let json = r#"[{"model":"m","params_b":1.0,"protocol":"ConstrainedJson",
            "measured_level":2,"tier_from_params":"Nano","tier_from_measured":"Small"}]"#;
        let recs: Vec<ModelProfileRecord> = serde_json::from_str(json).unwrap();
        assert_eq!(recs[0].calibrated_ring, None);
        assert_eq!(recs[0].measured_level, Some(2));
        assert_eq!(recs[0].calibration_evidence, None);
    }

    #[test]
    fn eligible_repeated_trial_evidence_is_recorded_in_the_profile() {
        let evidence = CalibrationEvidence {
            run_id: "run-1".to_string(),
            summary_file: "summary-run-1.json".to_string(),
            completed_at_unix_ms: 42,
            trials: 3,
            min_pass_rate: 0.9,
            required_passes: 3,
            qualified_levels: vec![0, 1, 3],
            full_ladder: true,
            complete: true,
            infrastructure_clean: true,
            eligible: true,
            measured_level: Some(1),
            ineligible_reason: None,
            diagnostic: false,
            budget_controls: None,
        };
        let record = calibrate_from_evidence("model", 7.0, "ConstrainedJson", &evidence).unwrap();
        assert_eq!(record.measured_level, Some(1));
        assert_eq!(record.calibration_evidence.as_ref(), Some(&evidence));
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), &record).unwrap();
        assert_eq!(
            read_profile(dir.path(), "model", "ConstrainedJson"),
            Some(record)
        );

        let mut inconsistent = evidence;
        inconsistent.measured_level = Some(3);
        assert!(
            calibrate_from_evidence("model", 7.0, "ConstrainedJson", &inconsistent).is_none(),
            "profile calibration must reject evidence that is not its own contiguous prefix"
        );
        inconsistent.measured_level = Some(1);
        inconsistent.required_passes = 2;
        assert!(
            calibrate_from_evidence("model", 7.0, "ConstrainedJson", &inconsistent).is_none(),
            "profile calibration must reject inconsistent numeric evidence"
        );
    }

    // --- ADR-086: the ladder is not always monotonic, and a partial sweep is
    // not a calibration ---

    /// The measured case: qwen2.5-coder-7b failed L5 and passed L6 in one full
    /// sweep, then passed L5 twice on repeat. The old `max(passed)` score hid
    /// that contradiction; the diagnostic remains useful after fixing it.
    #[test]
    fn a_failure_below_the_highest_pass_is_reported() {
        let rows = vec![row(0, true), row(4, true), row(5, false), row(6, true)];
        assert_eq!(highest_completed_level(&rows), Some(6));
        assert_eq!(
            non_monotonic_failures(&rows),
            vec![5],
            "L5 failed below the highest pass and must be surfaced"
        );
    }

    #[test]
    fn a_clean_ladder_reports_nothing() {
        let rows = vec![row(0, true), row(1, true), row(2, true)];
        assert!(non_monotonic_failures(&rows).is_empty());
    }

    /// Failures ABOVE the highest pass are the normal shape of a ceiling, not an
    /// inconsistency — they must not be reported as one.
    #[test]
    fn failures_above_the_ceiling_are_not_inconsistent() {
        let rows = vec![row(0, true), row(4, true), row(5, false), row(6, false)];
        assert_eq!(highest_completed_level(&rows), Some(4));
        assert!(
            non_monotonic_failures(&rows).is_empty(),
            "a plain ceiling at L4 is expected, not a contradiction"
        );
    }

    #[test]
    fn nothing_passed_means_nothing_to_report() {
        let rows = vec![row(0, false), row(1, false)];
        assert_eq!(highest_completed_level(&rows), None);
        assert!(non_monotonic_failures(&rows).is_empty());
    }

    #[test]
    fn calibration_stops_at_the_first_failed_or_missing_level() {
        let rows = vec![row(0, true), row(1, true), row(2, false), row(3, true)];
        let rec = calibrate("model", 1.0, "ConstrainedJson", &rows);
        assert_eq!(rec.measured_level, Some(1));
        assert_eq!(highest_completed_level(&rows), Some(3));
    }
}
