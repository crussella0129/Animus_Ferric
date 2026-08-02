//! Per-invocation benchmark summaries and calibration evidence.
//!
//! `results.jsonl` remains the append-only attempt ledger. A `RunSummary`
//! groups only the rows attributable to one run id, records the evidence used
//! for qualification, and is persisted as `summary-<run-id>.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::results::ResultRow;
use crate::verify::CommandCheckStatus;

pub const SUMMARY_SCHEMA_VERSION: u32 = 1;
const WILSON_Z_95: f64 = 1.959_963_984_540_054;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SampleStats {
    pub samples: u32,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    /// Sample standard deviation (`n - 1` denominator); zero for one sample.
    pub standard_deviation: Option<f64>,
    /// 75th percentile minus 25th percentile, using linear interpolation.
    pub iqr: Option<f64>,
}

impl SampleStats {
    pub fn from_values(values: impl IntoIterator<Item = f64>) -> Self {
        let mut values: Vec<f64> = values.into_iter().collect();
        values.sort_by(f64::total_cmp);
        if values.is_empty() {
            return Self {
                samples: 0,
                mean: None,
                median: None,
                standard_deviation: None,
                iqr: None,
            };
        }

        let samples = values.len() as u32;
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let standard_deviation = if values.len() == 1 {
            0.0
        } else {
            let squared_error = values
                .iter()
                .map(|value| (value - mean).powi(2))
                .sum::<f64>();
            (squared_error / (values.len() - 1) as f64).sqrt()
        };
        let q1 = percentile(&values, 0.25);
        let q3 = percentile(&values, 0.75);

        Self {
            samples,
            mean: Some(mean),
            median: Some(percentile(&values, 0.5)),
            standard_deviation: Some(standard_deviation),
            iqr: Some(q3 - q1),
        }
    }
}

fn percentile(sorted: &[f64], probability: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    let position = (sorted.len() - 1) as f64 * probability;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let fraction = position - lower as f64;
        sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Wilson95 {
    pub lower: f64,
    pub upper: f64,
}

impl Wilson95 {
    pub fn from_counts(passes: u32, samples: u32) -> Self {
        if samples == 0 {
            return Self {
                lower: 0.0,
                upper: 1.0,
            };
        }

        let n = f64::from(samples);
        let proportion = f64::from(passes) / n;
        let z_squared = WILSON_Z_95.powi(2);
        let denominator = 1.0 + z_squared / n;
        let center = proportion + z_squared / (2.0 * n);
        let margin =
            WILSON_Z_95 * ((proportion * (1.0 - proportion) + z_squared / (4.0 * n)) / n).sqrt();
        Self {
            lower: ((center - margin) / denominator).clamp(0.0, 1.0),
            upper: ((center + margin) / denominator).clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LevelSummary {
    pub level: u8,
    pub level_name: String,
    pub spec_version: u32,
    pub trials_expected: u32,
    pub trials_observed: u32,
    pub passes: u32,
    pub failures: u32,
    pub pass_rate: f64,
    pub required_passes: u32,
    pub qualified: bool,
    pub wilson_95: Wilson95,
    pub turns: SampleStats,
    pub input_tokens: SampleStats,
    pub output_tokens: SampleStats,
    pub wall_ms: SampleStats,
    /// Counts by observed terminal name; missing terminals use `missing`.
    pub terminal_counts: BTreeMap<String, u32>,
    /// Non-exclusive failure categories, so one attempt can increment several.
    pub failure_counts: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BinaryProvenance {
    pub path: String,
    pub size_bytes: Option<u64>,
    pub modified_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProvenance {
    pub backend: String,
    pub model: Option<String>,
    pub api_base: Option<String>,
    pub params_b: f32,
    pub ctx: u32,
    /// Operator-supplied or locally computed model artifact digest. Remote
    /// model IDs cannot be honestly converted into a file hash, so unknown is
    /// represented as null rather than inferred.
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunProvenance {
    pub ferric_version: String,
    /// Populated only when the build supplies a commit environment variable.
    pub git_commit: Option<String>,
    pub binary: BinaryProvenance,
    pub model: ModelProvenance,
    pub protocol: String,
    pub variant: String,
    pub python_bin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunIssue {
    pub trial_id: Option<String>,
    pub level: Option<u8>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationEvidence {
    pub run_id: String,
    pub summary_file: String,
    pub completed_at_unix_ms: u64,
    pub trials: u32,
    pub min_pass_rate: f64,
    pub required_passes: u32,
    pub qualified_levels: Vec<u8>,
    pub full_ladder: bool,
    pub complete: bool,
    pub infrastructure_clean: bool,
    pub eligible: bool,
    /// Longest qualified contiguous prefix beginning at L0.
    pub measured_level: Option<u8>,
    pub ineligible_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunSummary {
    pub schema_version: u32,
    pub run_id: String,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    pub trials_requested: u32,
    pub trials_completed: u32,
    pub min_pass_rate: f64,
    pub expected_levels: Vec<u8>,
    pub expected_rows: u32,
    pub observed_rows: u32,
    pub full_ladder: bool,
    pub complete: bool,
    pub infrastructure_clean: bool,
    pub provenance: RunProvenance,
    pub issues: Vec<RunIssue>,
    pub levels: Vec<LevelSummary>,
    pub calibration: CalibrationEvidence,
}

#[allow(clippy::too_many_arguments)]
pub fn summarize_run(
    run_id: &str,
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
    trials: u32,
    min_pass_rate: f64,
    expected_levels: &[u8],
    full_ladder: bool,
    rows: &[ResultRow],
    issues: Vec<RunIssue>,
    provenance: RunProvenance,
) -> RunSummary {
    let run_rows: Vec<&ResultRow> = rows
        .iter()
        .filter(|row| row.run_id.as_deref() == Some(run_id))
        .collect();
    let mut levels = expected_levels.to_vec();
    levels.sort_unstable();
    levels.dedup();
    let required = required_passes(trials, min_pass_rate);
    let level_summaries: Vec<LevelSummary> = levels
        .iter()
        .map(|level| summarize_level(*level, trials, required, &run_rows))
        .collect();

    let trials_completed = complete_trial_count(run_id, &levels, &run_rows);
    let expected_rows = trials.saturating_mul(levels.len() as u32);
    let complete = run_rows.len() as u32 == expected_rows && trials_completed == trials;
    let infrastructure_clean = issues.is_empty()
        && run_rows.iter().all(|row| {
            row.infrastructure_error.is_none()
                && !row
                    .command_checks
                    .iter()
                    .any(|check| check.status == CommandCheckStatus::InfrastructureError)
        });
    let qualified_levels: Vec<u8> = level_summaries
        .iter()
        .filter(|level| level.qualified)
        .map(|level| level.level)
        .collect();
    let eligible = full_ladder && complete && infrastructure_clean;
    let measured_level = eligible
        .then(|| contiguous_prefix(&qualified_levels))
        .flatten();
    let ineligible_reason = if !full_ladder {
        Some("partial ladder".to_string())
    } else if !complete {
        Some("incomplete trial matrix".to_string())
    } else if !infrastructure_clean {
        Some("benchmark infrastructure error".to_string())
    } else {
        None
    };
    let summary_file = format!("summary-{run_id}.json");

    RunSummary {
        schema_version: SUMMARY_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        started_at_unix_ms,
        finished_at_unix_ms,
        trials_requested: trials,
        trials_completed,
        min_pass_rate,
        expected_levels: levels,
        expected_rows,
        observed_rows: run_rows.len() as u32,
        full_ladder,
        complete,
        infrastructure_clean,
        provenance,
        issues,
        levels: level_summaries,
        calibration: CalibrationEvidence {
            run_id: run_id.to_string(),
            summary_file,
            completed_at_unix_ms: finished_at_unix_ms,
            trials,
            min_pass_rate,
            required_passes: required,
            qualified_levels,
            full_ladder,
            complete,
            infrastructure_clean,
            eligible,
            measured_level,
            ineligible_reason,
        },
    }
}

pub fn write_summary(dir: &Path, summary: &RunSummary) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(&summary.calibration.summary_file);
    let text = serde_json::to_string_pretty(summary).map_err(std::io::Error::other)?;
    std::fs::write(&path, text)?;
    Ok(path)
}

pub fn required_passes(trials: u32, min_pass_rate: f64) -> u32 {
    if trials == 0 {
        return 0;
    }
    (0..=trials)
        .find(|passes| f64::from(*passes) / f64::from(trials) >= min_pass_rate)
        .unwrap_or_else(|| trials.saturating_add(1))
}

fn summarize_level(level: u8, trials: u32, required: u32, rows: &[&ResultRow]) -> LevelSummary {
    let rows: Vec<&ResultRow> = rows
        .iter()
        .copied()
        .filter(|row| row.level == level)
        .collect();
    let observed = rows.len() as u32;
    let passes = rows.iter().filter(|row| row.completed).count() as u32;
    let pass_rate = if observed == 0 {
        0.0
    } else {
        f64::from(passes) / f64::from(observed)
    };
    let mut terminal_counts = BTreeMap::new();
    let mut failure_counts = BTreeMap::new();
    for row in &rows {
        increment(
            &mut terminal_counts,
            row.terminator.as_deref().unwrap_or("missing"),
        );
        if !row.completed {
            increment(&mut failure_counts, "incomplete");
        }
        if row.timed_out {
            increment(&mut failure_counts, "timed_out");
        }
        if row.exit_code != Some(0) {
            increment(&mut failure_counts, "nonzero_or_missing_exit");
        }
        if row.terminator.as_deref() != Some("task_complete") {
            increment(&mut failure_counts, "missing_task_complete");
        }
        if !row.expectations_ok {
            increment(&mut failure_counts, "expectations");
        }
        if !row.tools_ok {
            increment(&mut failure_counts, "tools");
        }
        if row.failure_admission.is_some() {
            increment(&mut failure_counts, "failure_admission");
        }
        if row.infrastructure_error.is_some() {
            increment(&mut failure_counts, "infrastructure");
        }
        for check in &row.command_checks {
            match check.status {
                CommandCheckStatus::Passed => {}
                CommandCheckStatus::ModelFailure => {
                    increment(&mut failure_counts, "command_check_model_failure");
                }
                CommandCheckStatus::InfrastructureError => {
                    increment(&mut failure_counts, "command_check_infrastructure");
                }
            }
        }
    }

    let exemplar = rows.first().copied();
    LevelSummary {
        level,
        level_name: exemplar
            .map(|row| row.level_name.clone())
            .unwrap_or_else(|| format!("L{level}")),
        spec_version: exemplar.map_or(1, |row| row.spec_version),
        trials_expected: trials,
        trials_observed: observed,
        passes,
        failures: observed.saturating_sub(passes),
        pass_rate,
        required_passes: required,
        qualified: observed == trials && passes >= required,
        wilson_95: Wilson95::from_counts(passes, observed),
        turns: SampleStats::from_values(rows.iter().map(|row| f64::from(row.turns))),
        input_tokens: SampleStats::from_values(rows.iter().map(|row| row.input_tokens as f64)),
        output_tokens: SampleStats::from_values(rows.iter().map(|row| row.output_tokens as f64)),
        wall_ms: SampleStats::from_values(rows.iter().map(|row| row.wall_ms as f64)),
        terminal_counts,
        failure_counts,
    }
}

fn complete_trial_count(run_id: &str, expected_levels: &[u8], rows: &[&ResultRow]) -> u32 {
    let expected: BTreeSet<u8> = expected_levels.iter().copied().collect();
    let mut trials: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
    for row in rows {
        if row.run_id.as_deref() != Some(run_id) {
            continue;
        }
        let Some(trial_id) = row.trial_id.as_deref() else {
            continue;
        };
        trials.entry(trial_id).or_default().push(row.level);
    }
    trials
        .values()
        .filter(|levels| {
            levels.len() == expected.len()
                && levels.iter().copied().collect::<BTreeSet<_>>() == expected
        })
        .count() as u32
}

fn contiguous_prefix(qualified_levels: &[u8]) -> Option<u8> {
    let qualified: BTreeSet<u8> = qualified_levels.iter().copied().collect();
    let mut level = 0_u8;
    while qualified.contains(&level) {
        if level == u8::MAX {
            return Some(level);
        }
        level += 1;
    }
    level.checked_sub(1)
}

fn increment(counts: &mut BTreeMap<String, u32>, key: &str) {
    *counts.entry(key.to_string()).or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(run: &str, trial: &str, level: u8, completed: bool, wall_ms: u64) -> ResultRow {
        ResultRow {
            run_id: Some(run.to_string()),
            trial_id: Some(trial.to_string()),
            started_at_unix_ms: Some(1),
            finished_at_unix_ms: Some(2),
            trace_path: None,
            infrastructure_error: None,
            level,
            spec_version: 1,
            level_name: format!("L{level}"),
            variant: "test".to_string(),
            protocol: "ConstrainedJson".to_string(),
            model: Some("model".to_string()),
            completed,
            timed_out: false,
            exit_code: Some(0),
            turns: 1,
            input_tokens: 2,
            output_tokens: 3,
            wall_ms,
            terminator: Some("task_complete".to_string()),
            tier_observed: None,
            protocol_observed: None,
            repetition_guard_fires: 0,
            tools_called: Vec::new(),
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

    fn provenance() -> RunProvenance {
        RunProvenance {
            ferric_version: "0.1.0".to_string(),
            git_commit: None,
            binary: BinaryProvenance {
                path: "ferric".to_string(),
                size_bytes: None,
                modified_at_unix_ms: None,
                sha256: None,
            },
            model: ModelProvenance {
                backend: "mock".to_string(),
                model: None,
                api_base: None,
                params_b: 1.2,
                ctx: 4096,
                sha256: None,
            },
            protocol: "ConstrainedJson".to_string(),
            variant: "test".to_string(),
            python_bin: "python".to_string(),
        }
    }

    #[test]
    fn sample_stats_use_sample_sd_and_interpolated_iqr() {
        let stats = SampleStats::from_values([1.0, 2.0, 3.0, 4.0]);
        assert_eq!(stats.mean, Some(2.5));
        assert_eq!(stats.median, Some(2.5));
        assert!((stats.standard_deviation.unwrap() - 1.290_994_448_735_805_6).abs() < 1e-12);
        assert_eq!(stats.iqr, Some(1.5));
    }

    #[test]
    fn wilson_interval_matches_nine_of_ten() {
        let interval = Wilson95::from_counts(9, 10);
        assert!((interval.lower - 0.595_849_973_204_761_5).abs() < 1e-12);
        assert!((interval.upper - 0.982_123_786_904_927).abs() < 1e-12);
    }

    #[test]
    fn required_passes_does_not_promote_decimal_roundoff() {
        assert_eq!(required_passes(100, 0.07), 7);
        assert_eq!(required_passes(3, 0.90), 3);
        assert_eq!(required_passes(3, 2.0 / 3.0), 2);
        assert_eq!(required_passes(10, 0.700_000_000_000_000_1), 8);
    }

    #[test]
    fn qualification_uses_ceiling_and_calibration_stops_at_first_gap() {
        let rows = vec![
            row("run", "trial-001", 0, true, 10),
            row("run", "trial-001", 1, true, 20),
            row("run", "trial-001", 2, true, 30),
            row("run", "trial-002", 0, true, 11),
            row("run", "trial-002", 1, false, 21),
            row("run", "trial-002", 2, true, 31),
            row("run", "trial-003", 0, true, 12),
            row("run", "trial-003", 1, true, 22),
            row("run", "trial-003", 2, true, 32),
        ];
        let summary = summarize_run(
            "run",
            1,
            2,
            3,
            0.9,
            &[0, 1, 2],
            true,
            &rows,
            Vec::new(),
            provenance(),
        );
        assert_eq!(summary.levels[0].required_passes, 3);
        assert!(summary.levels[0].qualified);
        assert!(!summary.levels[1].qualified);
        assert!(summary.levels[2].qualified);
        assert_eq!(summary.calibration.measured_level, Some(0));
        assert!(summary.calibration.eligible);
    }

    #[test]
    fn incomplete_or_infrastructure_dirty_run_is_not_calibration_evidence() {
        let rows = vec![row("run", "trial-001", 0, true, 10)];
        let incomplete = summarize_run(
            "run",
            1,
            2,
            2,
            0.9,
            &[0],
            true,
            &rows,
            Vec::new(),
            provenance(),
        );
        assert!(!incomplete.complete);
        assert!(!incomplete.calibration.eligible);

        let dirty = summarize_run(
            "run",
            1,
            2,
            1,
            0.9,
            &[0],
            true,
            &rows,
            vec![RunIssue {
                trial_id: Some("trial-001".to_string()),
                level: Some(0),
                message: "trace copy failed".to_string(),
            }],
            provenance(),
        );
        assert!(dirty.complete);
        assert!(!dirty.infrastructure_clean);
        assert!(!dirty.calibration.eligible);
    }

    #[test]
    fn rows_from_other_run_ids_do_not_pollute_the_summary() {
        let rows = vec![
            row("target", "trial-001", 0, true, 10),
            row("other", "trial-001", 0, false, 99),
        ];
        let summary = summarize_run(
            "target",
            1,
            2,
            1,
            1.0,
            &[0],
            false,
            &rows,
            Vec::new(),
            provenance(),
        );
        assert_eq!(summary.observed_rows, 1);
        assert_eq!(summary.levels[0].passes, 1);
        assert_eq!(summary.levels[0].wall_ms.mean, Some(10.0));
        assert!(summary.complete);
    }

    #[test]
    fn terminal_and_failure_counts_are_nonexclusive_and_attributable() {
        let mut failed = row("run", "trial-001", 0, false, 10);
        failed.timed_out = true;
        failed.exit_code = None;
        failed.terminator = None;
        failed.expectations_ok = false;
        failed.tools_ok = false;
        failed.failure_admission = Some("unable".to_string());
        failed
            .command_checks
            .push(crate::verify::CommandCheckResult {
                name: "behavior".to_string(),
                status: CommandCheckStatus::ModelFailure,
                exit_code: Some(1),
                timed_out: false,
                stdout_excerpt: String::new(),
                stderr_excerpt: String::new(),
                reason: Some("wrong output".to_string()),
            });
        let summary = summarize_run(
            "run",
            1,
            2,
            1,
            1.0,
            &[0],
            false,
            &[failed],
            Vec::new(),
            provenance(),
        );
        let level = &summary.levels[0];
        assert_eq!(level.terminal_counts["missing"], 1);
        for category in [
            "incomplete",
            "timed_out",
            "nonzero_or_missing_exit",
            "missing_task_complete",
            "expectations",
            "tools",
            "failure_admission",
            "command_check_model_failure",
        ] {
            assert_eq!(level.failure_counts[category], 1, "{category}");
        }
    }
}
