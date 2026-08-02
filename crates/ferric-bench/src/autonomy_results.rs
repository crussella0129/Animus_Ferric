//! Attributable results and statistical summaries for the internal autonomy matrix.
//!
//! These records intentionally distinguish contract compliance (for example,
//! safely pausing for required input) from objective completion. Neither rate is
//! an external benchmark score or a promotion signal.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::summary::{RunProvenance, SampleStats, Wilson95};
use crate::verify::{CommandCheckResult, CommandCheckStatus};

pub const AUTONOMY_RESULTS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomySegmentResult {
    pub segment: u32,
    pub expected_terminal: String,
    pub observed_terminal: Option<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub wall_ms: u64,
    /// Results-directory-relative retained trace path.
    pub trace_path: Option<String>,
    pub trace_sha256: Option<String>,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeProbeResult {
    pub mode: String,
    pub attempted: bool,
    pub rejected: bool,
    pub exit_code: Option<i32>,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyTraceMetrics {
    pub turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u32,
    pub tool_errors: u32,
    pub mutations: u32,
    pub verification_passes: u32,
    pub completion_gates_passed: u32,
    pub completion_gates_blocked: u32,
    pub repetition_guard_stops: u32,
    pub no_progress_guard_stops: u32,
    pub failure_guard_stops: u32,
    pub oscillation_guard_stops: u32,
    pub truncations: u32,
    pub provider_error_stops: u32,
    pub history_compactions: u32,
    /// First absolute turn where at least 80% of this episode's tool calls had
    /// occurred. `None` when the episode made no tool calls.
    pub horizon_80_turn: Option<u32>,
    pub tools_called: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutonomyResultRow {
    pub schema_version: u32,
    pub run_id: String,
    pub suite_id: String,
    pub suite_schema_version: u32,
    pub suite_sha256: String,
    pub task_id: String,
    pub task_name: String,
    pub category: String,
    pub variant: String,
    pub trial: u32,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    /// The observed terminal sequence, refusal probes, and clarification
    /// behavior matched the frozen task contract.
    pub contract_passed: bool,
    /// The requested repository end state passed authoritative checks after an
    /// exact successful terminal. A safe pause may pass the contract while this
    /// remains false.
    pub objective_completed: bool,
    pub infrastructure_error: Option<String>,
    pub final_terminal: Option<String>,
    pub segments: Vec<AutonomySegmentResult>,
    pub clarification_expected: bool,
    pub clarification_observed: bool,
    pub clarification_correct: bool,
    pub unnecessary_clarification: bool,
    pub resumes_expected: u32,
    pub resumes_observed: u32,
    pub recovery_succeeded: bool,
    pub refusal_probes: Vec<ResumeProbeResult>,
    /// `Some(true)` means the task's authoritative contract checked its
    /// idempotency/collateral-effect bound. `None` means the corpus did not
    /// produce that measurement; it is never inferred from a passing task.
    /// Reserved for a future effect-aware corpus. The v1 internal suite emits
    /// `None` and does not include this field in its contract score.
    pub duplicate_effects_within_limit: Option<bool>,
    pub command_checks: Vec<CommandCheckResult>,
    pub metrics: AutonomyTraceMetrics,
    pub wall_ms: u64,
    pub repository_brief_sha256: Option<String>,
    pub repository_brief_bytes: Option<u64>,
    pub repository_brief_truncated: Option<bool>,
    pub server_state: String,
    pub provenance: RunProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutonomyRateSummary {
    pub key: String,
    pub expected: u32,
    pub observed: u32,
    /// Rows without harness/infrastructure errors. Model rates use this
    /// denominator; infrastructure failures remain separately attributable.
    pub scoreable: u32,
    pub infrastructure_failures: u32,
    pub contract_passes: u32,
    pub objective_completions: u32,
    pub contract_rate: f64,
    pub objective_rate: f64,
    pub contract_wilson_95: Wilson95,
    pub objective_wilson_95: Wilson95,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutonomyToolSummary {
    pub tool: String,
    /// Scoreable episodes in which the model called this tool at least once.
    pub episodes_used: u32,
    /// Episodes in that conditional sample which completed the objective.
    pub objective_completions_when_used: u32,
    /// Conditional association, not tool-call accuracy or causal tool impact.
    pub objective_completion_rate_when_used: f64,
    pub objective_wilson_95_when_used: Wilson95,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClarificationSummary {
    pub required: u32,
    pub observed: u32,
    pub correct: u32,
    pub missed: u32,
    pub unnecessary: u32,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoverySummary {
    pub resumes_expected: u32,
    pub resumes_observed: u32,
    pub episodes_expected: u32,
    pub episodes_succeeded: u32,
    pub success_rate: f64,
    pub wilson_95: Wilson95,
    pub refusal_probes: u32,
    pub refusal_probes_rejected: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PassPowerSummary {
    /// Task/variant groups with at least three trials.
    pub eligible_groups: u32,
    /// Groups whose first three trials all completed the objective.
    pub successful_groups: u32,
    pub rate: Option<f64>,
    pub wilson_95: Option<Wilson95>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepositoryBriefComparison {
    pub paired_episodes: u32,
    pub recovery_wins: u32,
    pub repository_brief_wins: u32,
    pub both_completed: u32,
    pub neither_completed: u32,
    /// Repository-brief objective rate minus recovery objective rate on the
    /// exact paired sample.
    pub objective_rate_delta: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyRunIssue {
    pub task_id: Option<String>,
    pub variant: Option<String>,
    pub trial: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutonomyRunSummary {
    pub schema_version: u32,
    pub run_id: String,
    pub suite_id: String,
    pub suite_schema_version: u32,
    pub suite_sha256: String,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    pub trials_requested: u32,
    pub expected_tasks: Vec<String>,
    pub expected_variants: Vec<String>,
    pub expected_rows: u32,
    pub observed_rows: u32,
    pub complete: bool,
    pub infrastructure_clean: bool,
    pub internal_baseline_only: bool,
    pub provenance: Option<RunProvenance>,
    pub server_states: Vec<String>,
    pub issues: Vec<AutonomyRunIssue>,
    pub overall: AutonomyRateSummary,
    pub by_task: Vec<AutonomyRateSummary>,
    pub by_task_variant: Vec<AutonomyRateSummary>,
    pub by_category: Vec<AutonomyRateSummary>,
    pub by_variant: Vec<AutonomyRateSummary>,
    pub by_tool: Vec<AutonomyToolSummary>,
    pub clarification: ClarificationSummary,
    pub recovery: RecoverySummary,
    pub resolved_at_1: AutonomyRateSummary,
    pub pass_power_3: PassPowerSummary,
    pub repository_brief_ab: RepositoryBriefComparison,
    pub turns: SampleStats,
    pub input_tokens: SampleStats,
    pub output_tokens: SampleStats,
    pub tool_calls: SampleStats,
    pub wall_ms: SampleStats,
    pub horizon_80_turn: SampleStats,
    pub terminal_counts: BTreeMap<String, u32>,
    pub failure_counts: BTreeMap<String, u32>,
}

pub fn append_autonomy_row(dir: &Path, row: &AutonomyResultRow) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("autonomy-results.jsonl"))?;
    serde_json::to_writer(&mut file, row).map_err(std::io::Error::other)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn read_autonomy_rows(dir: &Path) -> std::io::Result<Vec<AutonomyResultRow>> {
    let text = std::fs::read_to_string(dir.join("autonomy-results.jsonl"))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid autonomy-results.jsonl line {}: {error}", index + 1),
                )
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn summarize_autonomy_run(
    run_id: &str,
    suite_id: &str,
    suite_schema_version: u32,
    suite_sha256: &str,
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
    trials_requested: u32,
    expected_tasks: &[String],
    expected_task_categories: &BTreeMap<String, String>,
    expected_variants: &[String],
    rows: &[AutonomyResultRow],
    issues: Vec<AutonomyRunIssue>,
) -> AutonomyRunSummary {
    let run_rows: Vec<&AutonomyResultRow> =
        rows.iter().filter(|row| row.run_id == run_id).collect();
    let expected_rows = trials_requested
        .saturating_mul(expected_tasks.len() as u32)
        .saturating_mul(expected_variants.len() as u32);
    let observed_coordinates: BTreeSet<_> = run_rows
        .iter()
        .map(|row| (row.task_id.clone(), row.variant.clone(), row.trial))
        .collect();
    let expected_coordinates: BTreeSet<_> = expected_tasks
        .iter()
        .flat_map(|task| {
            expected_variants.iter().flat_map(move |variant| {
                (1..=trials_requested).map(move |trial| (task.clone(), variant.clone(), trial))
            })
        })
        .collect();
    let expected_task_set: BTreeSet<_> = expected_tasks.iter().cloned().collect();
    let category_contract_matches = expected_task_categories.len() == expected_task_set.len()
        && expected_task_categories
            .keys()
            .all(|task| expected_task_set.contains(task));
    let row_contract_matches = run_rows.iter().all(|row| {
        row.schema_version == AUTONOMY_RESULTS_SCHEMA_VERSION
            && row.suite_id == suite_id
            && row.suite_schema_version == suite_schema_version
            && row.suite_sha256 == suite_sha256
            && expected_task_categories.get(&row.task_id) == Some(&row.category)
            && row.provenance.variant == row.variant
    });
    let provenance_matches = run_rows.first().is_none_or(|first| {
        run_rows.iter().all(|row| {
            let mut expected = first.provenance.clone();
            expected.variant = row.variant.clone();
            row.provenance == expected
        })
    });
    let complete = run_rows.len() as u32 == expected_rows
        && observed_coordinates == expected_coordinates
        && category_contract_matches
        && row_contract_matches
        && provenance_matches;
    let infrastructure_clean =
        issues.is_empty() && run_rows.iter().all(|row| row_is_scoreable(row));
    let provenance = run_rows.first().map(|row| {
        let mut provenance = row.provenance.clone();
        provenance.variant = "autonomy_matrix".to_string();
        provenance
    });
    let server_states: BTreeSet<_> = run_rows
        .iter()
        .map(|row| row.server_state.clone())
        .collect();

    let mut task_keys = expected_tasks.to_vec();
    task_keys.sort();
    task_keys.dedup();
    let mut variant_keys = expected_variants.to_vec();
    variant_keys.sort();
    variant_keys.dedup();
    let category_keys: BTreeSet<String> = expected_task_categories.values().cloned().collect();
    let scoreable_rows: Vec<_> = run_rows
        .iter()
        .copied()
        .filter(|row| row_is_scoreable(row))
        .collect();

    let overall = rate_summary("all", expected_rows, &run_rows);
    let by_task = task_keys
        .iter()
        .map(|key| {
            let selected: Vec<_> = run_rows
                .iter()
                .copied()
                .filter(|row| &row.task_id == key)
                .collect();
            rate_summary(
                key,
                trials_requested.saturating_mul(variant_keys.len() as u32),
                &selected,
            )
        })
        .collect();
    let by_task_variant = task_keys
        .iter()
        .flat_map(|task| {
            let rows = &run_rows;
            variant_keys.iter().map(move |variant| {
                let selected: Vec<_> = rows
                    .iter()
                    .copied()
                    .filter(|row| &row.task_id == task && &row.variant == variant)
                    .collect();
                rate_summary(&format!("{task}/{variant}"), trials_requested, &selected)
            })
        })
        .collect();
    let by_category = category_keys
        .iter()
        .map(|key| {
            let selected: Vec<_> = run_rows
                .iter()
                .copied()
                .filter(|row| &row.category == key)
                .collect();
            let tasks_in_category = task_keys
                .iter()
                .filter(|task| expected_task_categories.get(*task) == Some(key))
                .count() as u32;
            rate_summary(
                key,
                trials_requested
                    .saturating_mul(variant_keys.len() as u32)
                    .saturating_mul(tasks_in_category),
                &selected,
            )
        })
        .collect();
    let by_variant = variant_keys
        .iter()
        .map(|key| {
            let selected: Vec<_> = run_rows
                .iter()
                .copied()
                .filter(|row| &row.variant == key)
                .collect();
            rate_summary(
                key,
                trials_requested.saturating_mul(task_keys.len() as u32),
                &selected,
            )
        })
        .collect();
    let by_tool = summarize_tools(&run_rows);

    let required = scoreable_rows
        .iter()
        .filter(|row| row.clarification_expected)
        .count() as u32;
    let observed = scoreable_rows
        .iter()
        .filter(|row| row.clarification_observed)
        .count() as u32;
    let correct = scoreable_rows
        .iter()
        .filter(|row| row.clarification_correct)
        .count() as u32;
    let unnecessary = scoreable_rows
        .iter()
        .filter(|row| row.unnecessary_clarification)
        .count() as u32;
    let clarification = ClarificationSummary {
        required,
        observed,
        correct,
        missed: scoreable_rows
            .iter()
            .filter(|row| row.clarification_expected && !row.clarification_observed)
            .count() as u32,
        unnecessary,
        precision: ratio(correct, observed),
        recall: ratio(correct, required),
    };

    let recovery_rows: Vec<_> = scoreable_rows
        .iter()
        .copied()
        .filter(|row| row.resumes_expected > 0)
        .collect();
    let episodes_succeeded = recovery_rows
        .iter()
        .filter(|row| row.recovery_succeeded)
        .count() as u32;
    let probes = scoreable_rows
        .iter()
        .flat_map(|row| &row.refusal_probes)
        .filter(|probe| probe.attempted)
        .count() as u32;
    let probes_rejected = scoreable_rows
        .iter()
        .flat_map(|row| &row.refusal_probes)
        .filter(|probe| probe.attempted && probe.rejected)
        .count() as u32;
    let recovery = RecoverySummary {
        resumes_expected: scoreable_rows.iter().map(|row| row.resumes_expected).sum(),
        resumes_observed: scoreable_rows.iter().map(|row| row.resumes_observed).sum(),
        episodes_expected: recovery_rows.len() as u32,
        episodes_succeeded,
        success_rate: ratio(episodes_succeeded, recovery_rows.len() as u32).unwrap_or(0.0),
        wilson_95: Wilson95::from_counts(episodes_succeeded, recovery_rows.len() as u32),
        refusal_probes: probes,
        refusal_probes_rejected: probes_rejected,
    };

    let resolved_rows: Vec<_> = run_rows
        .iter()
        .copied()
        .filter(|row| row.trial == 1)
        .collect();
    let resolved_at_1 = rate_summary(
        "trial_1",
        (task_keys.len() as u32).saturating_mul(variant_keys.len() as u32),
        &resolved_rows,
    );
    let pass_power_3 = pass_power_3(&run_rows);
    let repository_brief_ab = repository_brief_comparison(&run_rows);

    let mut terminal_counts = BTreeMap::new();
    let mut failure_counts = BTreeMap::new();
    for row in &run_rows {
        increment(
            &mut terminal_counts,
            row.final_terminal.as_deref().unwrap_or("missing"),
        );
        if !row.contract_passed {
            increment(&mut failure_counts, "contract");
        }
        if !row.objective_completed {
            increment(&mut failure_counts, "objective_incomplete");
        }
        if row.infrastructure_error.is_some() {
            increment(&mut failure_counts, "infrastructure");
        }
        if row.unnecessary_clarification {
            increment(&mut failure_counts, "unnecessary_clarification");
        }
        if row.clarification_expected && !row.clarification_correct {
            increment(&mut failure_counts, "clarification_missed_or_incorrect");
        }
        if row.resumes_expected > 0 && !row.recovery_succeeded {
            increment(&mut failure_counts, "recovery");
        }
        if row
            .duplicate_effects_within_limit
            .is_some_and(|within| !within)
        {
            increment(&mut failure_counts, "duplicate_or_collateral_effect");
        }
        for check in &row.command_checks {
            match check.status {
                CommandCheckStatus::Passed => {}
                CommandCheckStatus::ModelFailure => increment(&mut failure_counts, "check"),
                CommandCheckStatus::InfrastructureError => {
                    increment(&mut failure_counts, "check_infrastructure")
                }
            }
        }
    }

    AutonomyRunSummary {
        schema_version: AUTONOMY_RESULTS_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        suite_id: suite_id.to_string(),
        suite_schema_version,
        suite_sha256: suite_sha256.to_string(),
        started_at_unix_ms,
        finished_at_unix_ms,
        trials_requested,
        expected_tasks: task_keys,
        expected_variants: variant_keys,
        expected_rows,
        observed_rows: run_rows.len() as u32,
        complete,
        infrastructure_clean,
        internal_baseline_only: true,
        provenance,
        server_states: server_states.into_iter().collect(),
        issues,
        overall,
        by_task,
        by_task_variant,
        by_category,
        by_variant,
        by_tool,
        clarification,
        recovery,
        resolved_at_1,
        pass_power_3,
        repository_brief_ab,
        turns: stats(&scoreable_rows, |row| f64::from(row.metrics.turns)),
        input_tokens: stats(&scoreable_rows, |row| row.metrics.input_tokens as f64),
        output_tokens: stats(&scoreable_rows, |row| row.metrics.output_tokens as f64),
        tool_calls: stats(&scoreable_rows, |row| f64::from(row.metrics.tool_calls)),
        wall_ms: stats(&scoreable_rows, |row| row.wall_ms as f64),
        horizon_80_turn: SampleStats::from_values(
            scoreable_rows
                .iter()
                .filter_map(|row| row.metrics.horizon_80_turn.map(f64::from)),
        ),
        terminal_counts,
        failure_counts,
    }
}

pub fn write_autonomy_summary(
    dir: &Path,
    summary: &AutonomyRunSummary,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("autonomy-summary-{}.json", summary.run_id));
    let text = serde_json::to_string_pretty(summary).map_err(std::io::Error::other)?;
    std::fs::write(&path, text)?;
    Ok(path)
}

fn rate_summary(key: &str, expected: u32, rows: &[&AutonomyResultRow]) -> AutonomyRateSummary {
    let observed = rows.len() as u32;
    let scoreable_rows: Vec<_> = rows
        .iter()
        .copied()
        .filter(|row| row_is_scoreable(row))
        .collect();
    let scoreable = scoreable_rows.len() as u32;
    let contract_passes = scoreable_rows
        .iter()
        .filter(|row| row.contract_passed)
        .count() as u32;
    let objective_completions = scoreable_rows
        .iter()
        .filter(|row| row.objective_completed)
        .count() as u32;
    AutonomyRateSummary {
        key: key.to_string(),
        expected,
        observed,
        scoreable,
        infrastructure_failures: observed.saturating_sub(scoreable),
        contract_passes,
        objective_completions,
        contract_rate: ratio(contract_passes, scoreable).unwrap_or(0.0),
        objective_rate: ratio(objective_completions, scoreable).unwrap_or(0.0),
        contract_wilson_95: Wilson95::from_counts(contract_passes, scoreable),
        objective_wilson_95: Wilson95::from_counts(objective_completions, scoreable),
    }
}

fn summarize_tools(rows: &[&AutonomyResultRow]) -> Vec<AutonomyToolSummary> {
    let mut counts: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for row in rows.iter().copied().filter(|row| row_is_scoreable(row)) {
        let tools: BTreeSet<_> = row.metrics.tools_called.iter().cloned().collect();
        for tool in tools {
            let entry = counts.entry(tool).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(u32::from(row.objective_completed));
        }
    }
    counts
        .into_iter()
        .map(
            |(tool, (episodes_used, objective_completions_when_used))| AutonomyToolSummary {
                tool,
                episodes_used,
                objective_completions_when_used,
                objective_completion_rate_when_used: ratio(
                    objective_completions_when_used,
                    episodes_used,
                )
                .unwrap_or(0.0),
                objective_wilson_95_when_used: Wilson95::from_counts(
                    objective_completions_when_used,
                    episodes_used,
                ),
            },
        )
        .collect()
}

fn pass_power_3(rows: &[&AutonomyResultRow]) -> PassPowerSummary {
    let mut groups: BTreeMap<(&str, &str), Vec<&AutonomyResultRow>> = BTreeMap::new();
    for row in rows {
        groups
            .entry((row.task_id.as_str(), row.variant.as_str()))
            .or_default()
            .push(row);
    }
    let mut eligible = 0_u32;
    let mut successful = 0_u32;
    for group in groups.values_mut() {
        group.sort_by_key(|row| row.trial);
        let first_three: Vec<_> = group
            .iter()
            .copied()
            .filter(|row| (1..=3).contains(&row.trial))
            .collect();
        if first_three.len() == 3
            && first_three.iter().map(|row| row.trial).eq([1, 2, 3])
            && first_three.iter().all(|row| row_is_scoreable(row))
        {
            eligible += 1;
            if first_three.iter().all(|row| row.objective_completed) {
                successful += 1;
            }
        }
    }
    PassPowerSummary {
        eligible_groups: eligible,
        successful_groups: successful,
        rate: ratio(successful, eligible),
        wilson_95: (eligible > 0).then(|| Wilson95::from_counts(successful, eligible)),
    }
}

fn repository_brief_comparison(rows: &[&AutonomyResultRow]) -> RepositoryBriefComparison {
    let mut recovery = BTreeMap::new();
    let mut brief = BTreeMap::new();
    for row in rows.iter().copied().filter(|row| row_is_scoreable(row)) {
        let key = (row.task_id.as_str(), row.trial);
        match row.variant.as_str() {
            "recovery" => {
                recovery.insert(key, row.objective_completed);
            }
            "repository_brief" => {
                brief.insert(key, row.objective_completed);
            }
            _ => {}
        }
    }
    let mut recovery_wins = 0_u32;
    let mut repository_brief_wins = 0_u32;
    let mut both_completed = 0_u32;
    let mut neither_completed = 0_u32;
    for (key, recovery_completed) in &recovery {
        let Some(brief_completed) = brief.get(key) else {
            continue;
        };
        match (*recovery_completed, *brief_completed) {
            (true, false) => recovery_wins += 1,
            (false, true) => repository_brief_wins += 1,
            (true, true) => both_completed += 1,
            (false, false) => neither_completed += 1,
        }
    }
    let paired_episodes = recovery_wins
        .saturating_add(repository_brief_wins)
        .saturating_add(both_completed)
        .saturating_add(neither_completed);
    let recovery_completed = recovery_wins.saturating_add(both_completed);
    let brief_completed = repository_brief_wins.saturating_add(both_completed);
    RepositoryBriefComparison {
        paired_episodes,
        recovery_wins,
        repository_brief_wins,
        both_completed,
        neither_completed,
        objective_rate_delta: (paired_episodes > 0).then(|| {
            (f64::from(brief_completed) - f64::from(recovery_completed))
                / f64::from(paired_episodes)
        }),
    }
}

fn stats(rows: &[&AutonomyResultRow], value: impl Fn(&AutonomyResultRow) -> f64) -> SampleStats {
    SampleStats::from_values(rows.iter().map(|row| value(row)))
}

fn row_is_scoreable(row: &AutonomyResultRow) -> bool {
    row.infrastructure_error.is_none()
        && row
            .command_checks
            .iter()
            .all(|check| check.status != CommandCheckStatus::InfrastructureError)
}

fn ratio(numerator: u32, denominator: u32) -> Option<f64> {
    (denominator > 0).then(|| f64::from(numerator) / f64::from(denominator))
}

fn increment(counts: &mut BTreeMap<String, u32>, key: &str) {
    *counts.entry(key.to_string()).or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::{BinaryProvenance, ModelProvenance};

    fn row(
        run_id: &str,
        task: &str,
        category: &str,
        variant: &str,
        trial: u32,
    ) -> AutonomyResultRow {
        AutonomyResultRow {
            schema_version: AUTONOMY_RESULTS_SCHEMA_VERSION,
            run_id: run_id.to_string(),
            suite_id: "autonomy-v1".to_string(),
            suite_schema_version: 1,
            suite_sha256: "suite".to_string(),
            task_id: task.to_string(),
            task_name: task.to_string(),
            category: category.to_string(),
            variant: variant.to_string(),
            trial,
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            contract_passed: true,
            objective_completed: true,
            infrastructure_error: None,
            final_terminal: Some("task_complete".to_string()),
            segments: vec![AutonomySegmentResult {
                segment: 1,
                expected_terminal: "completed".to_string(),
                observed_terminal: Some("task_complete".to_string()),
                exit_code: Some(0),
                timed_out: false,
                wall_ms: 10,
                trace_path: Some("traces/test.jsonl".to_string()),
                trace_sha256: Some("trace".to_string()),
                stderr_tail: String::new(),
            }],
            clarification_expected: false,
            clarification_observed: false,
            clarification_correct: false,
            unnecessary_clarification: false,
            resumes_expected: 0,
            resumes_observed: 0,
            recovery_succeeded: true,
            refusal_probes: Vec::new(),
            duplicate_effects_within_limit: None,
            command_checks: Vec::new(),
            metrics: AutonomyTraceMetrics {
                turns: 2,
                input_tokens: 10,
                output_tokens: 5,
                tool_calls: 2,
                horizon_80_turn: Some(1),
                ..Default::default()
            },
            wall_ms: 10,
            repository_brief_sha256: None,
            repository_brief_bytes: None,
            repository_brief_truncated: None,
            server_state: "unknown".to_string(),
            provenance: RunProvenance {
                ferric_version: "0.1.0".to_string(),
                git_commit: Some("abc".to_string()),
                binary: BinaryProvenance {
                    path: "ferric".to_string(),
                    size_bytes: Some(1),
                    modified_at_unix_ms: Some(1),
                    sha256: Some("binary".to_string()),
                },
                model: ModelProvenance {
                    backend: "mock".to_string(),
                    model: None,
                    api_base: None,
                    params_b: 1.2,
                    ctx: 4096,
                    sha256: None,
                },
                protocol: "unified_grammar".to_string(),
                variant: variant.to_string(),
                python_bin: "python".to_string(),
            },
        }
    }

    fn categories(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(task, category)| ((*task).to_string(), (*category).to_string()))
            .collect()
    }

    #[test]
    fn summary_filters_run_and_keeps_contract_distinct_from_completion() {
        let mut safe_pause = row("run", "A01", "ambiguity", "current", 1);
        safe_pause.objective_completed = false;
        safe_pause.final_terminal = Some("needs_input".to_string());
        safe_pause.clarification_expected = true;
        safe_pause.clarification_observed = true;
        safe_pause.clarification_correct = true;
        let foreign = row("other", "A01", "ambiguity", "current", 1);
        let summary = summarize_autonomy_run(
            "run",
            "autonomy-v1",
            1,
            "suite",
            1,
            2,
            1,
            &["A01".to_string()],
            &categories(&[("A01", "ambiguity")]),
            &["current".to_string()],
            &[safe_pause, foreign],
            Vec::new(),
        );
        assert!(summary.complete);
        assert_eq!(summary.overall.contract_passes, 1);
        assert_eq!(summary.overall.objective_completions, 0);
        assert_eq!(summary.clarification.precision, Some(1.0));
        assert_eq!(summary.clarification.recall, Some(1.0));
    }

    #[test]
    fn pass_power_three_requires_three_successes_in_one_task_variant() {
        let rows = [
            row("run", "H01", "long_horizon", "recovery", 1),
            row("run", "H01", "long_horizon", "recovery", 2),
            row("run", "H01", "long_horizon", "recovery", 3),
        ];
        let refs: Vec<_> = rows.iter().collect();
        let summary = pass_power_3(&refs);
        assert_eq!(summary.eligible_groups, 1);
        assert_eq!(summary.successful_groups, 1);
        assert_eq!(summary.rate, Some(1.0));
    }

    #[test]
    fn duplicate_coordinates_make_a_run_incomplete() {
        let rows = [
            row("run", "A01", "ambiguity", "current", 1),
            row("run", "A01", "ambiguity", "current", 1),
        ];
        let summary = summarize_autonomy_run(
            "run",
            "autonomy-v1",
            1,
            "suite",
            1,
            2,
            2,
            &["A01".to_string()],
            &categories(&[("A01", "ambiguity")]),
            &["current".to_string()],
            &rows,
            Vec::new(),
        );
        assert!(!summary.complete);
    }

    #[test]
    fn unknown_coordinate_cannot_replace_an_expected_row() {
        let rows = [row("run", "H99", "long_horizon", "current", 1)];
        let summary = summarize_autonomy_run(
            "run",
            "autonomy-v1",
            1,
            "suite",
            1,
            2,
            1,
            &["H01".to_string()],
            &categories(&[("H01", "long_horizon")]),
            &["current".to_string()],
            &rows,
            Vec::new(),
        );
        assert!(!summary.complete);
    }

    #[test]
    fn category_drift_cannot_complete_an_expected_coordinate() {
        let rows = [row("run", "H01", "ambiguity", "current", 1)];
        let summary = summarize_autonomy_run(
            "run",
            "autonomy-v1",
            1,
            "suite",
            1,
            2,
            1,
            &["H01".to_string()],
            &categories(&[("H01", "long_horizon")]),
            &["current".to_string()],
            &rows,
            Vec::new(),
        );
        assert!(!summary.complete);
        assert_eq!(summary.by_category[0].expected, 1);
        assert_eq!(summary.by_category[0].observed, 0);
    }

    #[test]
    fn infrastructure_rows_are_not_scored_as_model_failures() {
        let mut infrastructure = row("run", "H01", "long_horizon", "current", 1);
        infrastructure.contract_passed = false;
        infrastructure.objective_completed = false;
        infrastructure.infrastructure_error = Some("trace unavailable".to_string());
        let summary = rate_summary("test", 1, &[&infrastructure]);
        assert_eq!(summary.observed, 1);
        assert_eq!(summary.scoreable, 0);
        assert_eq!(summary.infrastructure_failures, 1);
        assert_eq!(summary.objective_completions, 0);
    }

    #[test]
    fn infrastructure_rows_do_not_enter_secondary_model_metrics() {
        let mut infrastructure = row("run", "R01", "recovery", "recovery", 1);
        infrastructure.contract_passed = false;
        infrastructure.objective_completed = false;
        infrastructure.infrastructure_error = Some("provider unavailable".to_string());
        infrastructure.clarification_expected = true;
        infrastructure.clarification_observed = true;
        infrastructure.clarification_correct = true;
        infrastructure.resumes_expected = 1;
        infrastructure.resumes_observed = 1;
        let summary = summarize_autonomy_run(
            "run",
            "autonomy-v1",
            1,
            "suite",
            1,
            2,
            1,
            &["R01".to_string()],
            &categories(&[("R01", "recovery")]),
            &["recovery".to_string()],
            &[infrastructure],
            Vec::new(),
        );
        assert_eq!(summary.overall.scoreable, 0);
        assert_eq!(summary.clarification.required, 0);
        assert_eq!(summary.recovery.episodes_expected, 0);
        assert_eq!(summary.turns.samples, 0);
    }

    #[test]
    fn malformed_jsonl_reports_its_physical_line() {
        let dir = tempfile::tempdir().unwrap();
        let valid = serde_json::to_string(&row("run", "A01", "ambiguity", "current", 1)).unwrap();
        std::fs::write(
            dir.path().join("autonomy-results.jsonl"),
            format!("{valid}\n\n{{bad}}\n"),
        )
        .unwrap();
        let error = read_autonomy_rows(dir.path()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 3"));
    }
}
