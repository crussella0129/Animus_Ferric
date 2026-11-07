//! Calibration (T-214, ADR-019): turn a sweep's results into a model's
//! `measured_level`, the input that overrides parameter-count tiering in
//! `ferric_core::policy_for`. `ferric bench` is the SOLE producer of this
//! value — tier assignments change only with a committed measurement diff.

use std::path::Path;

use ferric_core::{tier_for_level, tier_for_params};
use serde::{Deserialize, Serialize};

use crate::results::write_pretty;
use crate::summary::{CalibrationEvidence, longest_prefix, required_passes};

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

impl ModelProfileRecord {
    /// A record whose tiers derive from `params_b` and `measured_level`.
    fn new(model: &str, params_b: f32, protocol: &str, measured_level: Option<u8>) -> Self {
        Self {
            model: model.to_string(),
            params_b,
            protocol: protocol.to_string(),
            measured_level,
            tier_from_params: format!("{:?}", tier_for_params(params_b)),
            tier_from_measured: measured_level.map(|level| format!("{:?}", tier_for_level(level))),
            calibrated_ring: None,
            calibration_evidence: None,
        }
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
    let derived_level = longest_prefix(|level| evidence.qualified_levels.contains(&level));
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
        calibration_evidence: Some(evidence.clone()),
        ..ModelProfileRecord::new(model, params_b, protocol, derived_level)
    })
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
    let mut record = read_profile(dir, model, protocol)
        .unwrap_or_else(|| ModelProfileRecord::new(model, params_b, protocol, None));
    record.calibrated_ring = Some(ring);
    write_profile(dir, &record)
}

/// Write/replace this model+protocol's record in `<dir>/model_profiles.json`
/// (a JSON array; existing records for other model/protocol keys are kept).
pub fn write_profile(dir: &Path, record: &ModelProfileRecord) -> std::io::Result<()> {
    let mut records: Vec<ModelProfileRecord> =
        std::fs::read_to_string(dir.join("model_profiles.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
    records.retain(|r| !(r.model == record.model && r.protocol == record.protocol));
    records.push(record.clone());
    write_pretty(dir, "model_profiles.json", &records).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrate_records_both_tiers() {
        // A 1B that completes L4 → measured_level 4 → tier_for_level(4)=Small,
        // even though params (1.0) → Nano. This is the override in action.
        let rec = ModelProfileRecord::new("llama-1b", 1.0, "unified_grammar", Some(4));
        assert_eq!(rec.measured_level, Some(4));
        assert_eq!(rec.tier_from_params, "Nano");
        assert_eq!(rec.tier_from_measured.as_deref(), Some("Small"));
        let failed = ModelProfileRecord::new("weak", 1.0, "native_tools", None);
        assert_eq!(failed.tier_from_measured, None);
    }

    #[test]
    fn write_profile_replaces_same_key_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        for (model, params_b, level) in [
            ("llama-1b", 1.0, 0),
            ("qwen-7b", 7.0, 0),
            ("llama-1b", 1.0, 1),
        ] {
            let record = ModelProfileRecord::new(model, params_b, "unified_grammar", Some(level));
            write_profile(dir.path(), &record).unwrap();
        }

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
    fn read_profile_round_trips_and_misses_are_none() {
        let dir = tempfile::tempdir().unwrap();
        // Missing file → None.
        assert_eq!(read_profile(dir.path(), "m", "ConstrainedJson"), None);
        let rec = ModelProfileRecord::new("qwen-7b", 7.0, "ConstrainedJson", Some(0));
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
        let rec = ModelProfileRecord::new("llama-1b", 1.0, "ConstrainedJson", Some(4));
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
}
