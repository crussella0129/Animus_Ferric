//! Attributable results and statistical summaries for the internal autonomy matrix.
//!
//! These records intentionally distinguish contract compliance (for example,
//! safely pausing for required input) from objective completion. Neither rate is
//! an external benchmark score or a promotion signal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::provenance::is_sha256_hex;
use crate::results::{append_jsonl, write_pretty};
use crate::summary::{RunProvenance, SampleStats, Wilson95, increment};
use crate::verify::{CommandCheckResult, CommandCheckStatus};
use ferric_core::HarnessPolicy;

/// Version two added harness-policy and evaluation provenance. Every newer
/// field has a serde default, so historical version-one rows remain readable
/// as legacy evidence. Rows written by the retired paired runner (INT-0018)
/// also still parse: their arm, pair and strict managed-server fields are
/// ignored.
pub const AUTONOMY_RESULTS_SCHEMA_VERSION: u32 = 2;

/// Row-level provenance which was absent from schema v1. The default maps old
/// rows to their exact historical meaning: legacy policy, nothing recorded.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyEvaluationProvenance {
    #[serde(default)]
    pub harness_policy: HarnessPolicy,
    /// Digest of the canonical temporary-workspace identity. This identifies
    /// the episode's fresh workspace without retaining a machine-specific path.
    #[serde(default)]
    pub workspace_instance_sha256: Option<String>,
    /// Deterministic digest of sorted portable relative paths and file bytes,
    /// captured after task materialization and before any child execution.
    #[serde(default)]
    pub initial_tree_sha256: Option<String>,
    #[serde(default)]
    pub child_binary_sha256: Option<String>,
    #[serde(default)]
    pub corpus_sha256: Option<String>,
    #[serde(default)]
    pub model_sha256: Option<String>,
    /// Exact child-query literal, represented as text to avoid floating-point
    /// provenance ambiguity.
    #[serde(default)]
    pub query_temperature: Option<String>,
    #[serde(default)]
    pub managed_server: Option<ManagedServerProvenance>,
}

/// The `ferric server` registration the matrix endpoint was discovered from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedServerProvenance {
    pub engine: String,
    pub listener_base_url: String,
    pub model: Option<String>,
    pub context_size: Option<u32>,
    pub sampling_seed: Option<i64>,
    pub parallel_slots: Option<u32>,
}

/// A typed assertion that trace structure was validated from the same retained
/// byte snapshot whose path and SHA-256 are recorded in the segment row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetainedTraceValidation {
    StructureValidated,
}

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
    #[serde(default)]
    pub trace_validation: Option<RetainedTraceValidation>,
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
    #[serde(flatten)]
    pub mechanisms: MechanismCounts,
    /// First absolute turn where at least 80% of this episode's tool calls had
    /// occurred. `None` when the episode made no tool calls.
    pub horizon_80_turn: Option<u32>,
    pub tools_called: Vec<String>,
}

/// Evidence-controller mechanism counters. Flattened into both per-episode
/// metrics and per-policy summaries, so their JSON keys stay top-level.
/// Every counter defaults to zero for rows written before it existed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MechanismCounts {
    pub observations_recorded: u32,
    pub file_observations: u32,
    pub search_observations: u32,
    pub find_observations: u32,
    pub controller_blocks: u32,
    pub blind_mutation_blocks: u32,
    pub same_turn_observation_blocks: u32,
    pub stale_observation_blocks: u32,
    pub unsupported_mutation_blocks: u32,
    pub repair_inspection_blocks: u32,
    pub no_effect_blocks: u32,
    pub syntax_regression_blocks: u32,
    pub repeated_check_blocks: u32,
    pub workspace_effects_recorded: u32,
    pub workspace_effect_paths: u32,
    pub verification_checks_recorded: u32,
    pub verification_checks_passed: u32,
    pub verification_checks_failed: u32,
    pub verification_repair_attempts: u32,
    /// Distinct failed-diagnostic fingerprints. A policy summary sums the
    /// per-episode counts; digests are deliberately not copied into it.
    pub distinct_failed_diagnostic_fingerprints: u32,
    pub controller_checkpoints: u32,
    pub recovery_packets_injected: u32,
}

impl MechanismCounts {
    /// Saturating field-wise sum.
    pub fn add(&mut self, other: &Self) {
        macro_rules! add_fields {
            ($($field:ident),* $(,)?) => {
                $(self.$field = self.$field.saturating_add(other.$field);)*
            };
        }
        add_fields!(
            observations_recorded,
            file_observations,
            search_observations,
            find_observations,
            controller_blocks,
            blind_mutation_blocks,
            same_turn_observation_blocks,
            stale_observation_blocks,
            unsupported_mutation_blocks,
            repair_inspection_blocks,
            no_effect_blocks,
            syntax_regression_blocks,
            repeated_check_blocks,
            workspace_effects_recorded,
            workspace_effect_paths,
            verification_checks_recorded,
            verification_checks_passed,
            verification_checks_failed,
            verification_repair_attempts,
            distinct_failed_diagnostic_fingerprints,
            controller_checkpoints,
            recovery_packets_injected,
        );
    }
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
    #[serde(default)]
    pub harness_policy: HarnessPolicy,
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
    #[serde(default)]
    pub evaluation_provenance: AutonomyEvaluationProvenance,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyMechanismSummary {
    pub harness_policy: HarnessPolicy,
    pub scoreable_episodes: u32,
    #[serde(flatten)]
    pub mechanisms: MechanismCounts,
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
pub struct PolicyPassPowerSummary {
    pub harness_policy: HarnessPolicy,
    pub summary: PassPowerSummary,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyRepositoryBriefComparison {
    pub harness_policy: HarnessPolicy,
    pub comparison: RepositoryBriefComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyRunIssue {
    pub task_id: Option<String>,
    pub variant: Option<String>,
    #[serde(default)]
    pub harness_policy: Option<HarnessPolicy>,
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
    #[serde(default)]
    pub by_harness_policy: Vec<AutonomyRateSummary>,
    pub by_tool: Vec<AutonomyToolSummary>,
    #[serde(default)]
    pub by_policy_mechanisms: Vec<PolicyMechanismSummary>,
    pub clarification: ClarificationSummary,
    pub recovery: RecoverySummary,
    pub resolved_at_1: AutonomyRateSummary,
    pub pass_power_3: PassPowerSummary,
    #[serde(default)]
    pub pass_power_3_by_policy: Vec<PolicyPassPowerSummary>,
    pub repository_brief_ab: RepositoryBriefComparison,
    #[serde(default)]
    pub repository_brief_ab_by_policy: Vec<PolicyRepositoryBriefComparison>,
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
    append_jsonl(dir, "autonomy-results.jsonl", row)
}

/// The harness policy every matrix episode runs.
const MATRIX_POLICY: HarnessPolicy = HarnessPolicy::Legacy;

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
        .map(|row| {
            (
                row.task_id.clone(),
                row.variant.clone(),
                row.trial,
                row.harness_policy,
            )
        })
        .collect();
    let expected_row_coordinates: BTreeSet<_> = expected_tasks
        .iter()
        .flat_map(|task| {
            expected_variants.iter().flat_map(move |variant| {
                (1..=trials_requested)
                    .map(move |trial| (task.clone(), variant.clone(), trial, MATRIX_POLICY))
            })
        })
        .collect();
    let expected_task_set: BTreeSet<_> = expected_tasks.iter().cloned().collect();
    let expected_variant_set: BTreeSet<_> = expected_variants.iter().cloned().collect();
    let expected_dimensions_unique = trials_requested > 0
        && !expected_task_set.is_empty()
        && !expected_variant_set.is_empty()
        && expected_task_set.len() == expected_tasks.len()
        && expected_variant_set.len() == expected_variants.len();
    let category_contract_matches = expected_task_categories.len() == expected_task_set.len()
        && expected_task_categories
            .keys()
            .all(|task| expected_task_set.contains(task));
    let row_contract_matches = run_rows.iter().all(|row| {
        let schema_matches = if row.schema_version == AUTONOMY_RESULTS_SCHEMA_VERSION {
            row.evaluation_provenance.harness_policy == row.harness_policy
                && row.evaluation_provenance.child_binary_sha256 == row.provenance.binary.sha256
                && row.evaluation_provenance.corpus_sha256.as_deref() == Some(suite_sha256)
                && row.evaluation_provenance.model_sha256 == row.provenance.model.sha256
                && row.evaluation_provenance.query_temperature.as_deref() == Some("0.0")
        } else {
            row.schema_version == 1
                && row.harness_policy == HarnessPolicy::Legacy
                && row.evaluation_provenance == AutonomyEvaluationProvenance::default()
        };
        schema_matches
            && row.suite_id == suite_id
            && row.suite_schema_version == suite_schema_version
            && row.suite_sha256 == suite_sha256
            && expected_task_categories.get(&row.task_id) == Some(&row.category)
            && row.provenance.variant == row.variant
    });
    let provenance_matches = provenance_matches(&run_rows);
    let complete = run_rows.len() as u32 == expected_rows
        && observed_coordinates == expected_row_coordinates
        && category_contract_matches
        && row_contract_matches
        && provenance_matches
        && expected_dimensions_unique;
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
    let policy_keys: BTreeSet<_> = expected_row_coordinates
        .iter()
        .map(|coordinate| coordinate.3)
        .collect();
    let category_keys: BTreeSet<String> = expected_task_categories.values().cloned().collect();
    let scoreable_rows: Vec<_> = run_rows
        .iter()
        .copied()
        .filter(|row| row_is_scoreable(row))
        .collect();

    let overall = rate_summary("all", expected_rows, &run_rows, &scoreable_rows, |_| true);
    let by_task = task_keys
        .iter()
        .map(|key| {
            rate_summary(
                key,
                trials_requested.saturating_mul(variant_keys.len() as u32),
                &run_rows,
                &scoreable_rows,
                |row| &row.task_id == key,
            )
        })
        .collect();
    let by_task_variant = task_keys
        .iter()
        .flat_map(|task| {
            let rows = &run_rows;
            let scoreable = &scoreable_rows;
            variant_keys.iter().map(move |variant| {
                rate_summary(
                    &format!("{task}/{variant}"),
                    trials_requested,
                    rows,
                    scoreable,
                    |row| &row.task_id == task && &row.variant == variant,
                )
            })
        })
        .collect();
    let by_category = category_keys
        .iter()
        .map(|key| {
            let tasks_in_category = task_keys
                .iter()
                .filter(|task| expected_task_categories.get(*task) == Some(key))
                .count() as u32;
            rate_summary(
                key,
                trials_requested
                    .saturating_mul(variant_keys.len() as u32)
                    .saturating_mul(tasks_in_category),
                &run_rows,
                &scoreable_rows,
                |row| &row.category == key,
            )
        })
        .collect();
    let by_variant = variant_keys
        .iter()
        .map(|key| {
            rate_summary(
                key,
                trials_requested.saturating_mul(task_keys.len() as u32),
                &run_rows,
                &scoreable_rows,
                |row| &row.variant == key,
            )
        })
        .collect();
    let by_harness_policy = policy_keys
        .iter()
        .map(|policy| {
            rate_summary(
                policy.label(),
                trials_requested
                    .saturating_mul(task_keys.len() as u32)
                    .saturating_mul(variant_keys.len() as u32),
                &run_rows,
                &scoreable_rows,
                |row| row.harness_policy == *policy,
            )
        })
        .collect();
    let by_tool = summarize_tools(&scoreable_rows);
    let by_policy_mechanisms = policy_keys
        .iter()
        .map(|policy| summarize_policy_mechanisms(*policy, &scoreable_rows))
        .collect();

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

    let resolved_at_1 = rate_summary(
        "trial_1",
        (task_keys.len() as u32).saturating_mul(variant_keys.len() as u32),
        &run_rows,
        &scoreable_rows,
        |row| row.trial == 1,
    );
    let aggregate_pass_power_3 = pass_power_3(&scoreable_rows);
    let pass_power_3_by_policy = policy_keys
        .iter()
        .map(|policy| PolicyPassPowerSummary {
            harness_policy: *policy,
            summary: pass_power_3(
                &scoreable_rows
                    .iter()
                    .copied()
                    .filter(|row| row.harness_policy == *policy)
                    .collect::<Vec<_>>(),
            ),
        })
        .collect();
    let repository_brief_ab = repository_brief_comparison(&scoreable_rows);
    let repository_brief_ab_by_policy = policy_keys
        .iter()
        .map(|policy| PolicyRepositoryBriefComparison {
            harness_policy: *policy,
            comparison: repository_brief_comparison(
                &scoreable_rows
                    .iter()
                    .copied()
                    .filter(|row| row.harness_policy == *policy)
                    .collect::<Vec<_>>(),
            ),
        })
        .collect();
    let mut terminal_counts = BTreeMap::new();
    let mut failure_counts = BTreeMap::new();
    for row in &run_rows {
        increment(
            &mut terminal_counts,
            row.final_terminal.as_deref().unwrap_or("missing"),
        );
        if row.infrastructure_error.is_some() {
            increment(&mut failure_counts, "infrastructure");
        }
        for check in &row.command_checks {
            if check.status == CommandCheckStatus::InfrastructureError {
                increment(&mut failure_counts, "check_infrastructure");
            }
        }
    }
    for row in &scoreable_rows {
        if !row.contract_passed {
            increment(&mut failure_counts, "contract");
        }
        if !row.objective_completed {
            increment(&mut failure_counts, "objective_incomplete");
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
                CommandCheckStatus::InfrastructureError => {}
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
        by_harness_policy,
        by_tool,
        by_policy_mechanisms,
        clarification,
        recovery,
        resolved_at_1,
        pass_power_3: aggregate_pass_power_3,
        pass_power_3_by_policy,
        repository_brief_ab,
        repository_brief_ab_by_policy,
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
    write_pretty(
        dir,
        &format!("autonomy-summary-{}.json", summary.run_id),
        summary,
    )
}

fn rate_summary(
    key: &str,
    expected: u32,
    observed_rows: &[&AutonomyResultRow],
    scoreable_rows: &[&AutonomyResultRow],
    include: impl Fn(&AutonomyResultRow) -> bool,
) -> AutonomyRateSummary {
    let observed = observed_rows.iter().filter(|row| include(row)).count() as u32;
    let scoreable_rows: Vec<_> = scoreable_rows
        .iter()
        .copied()
        .filter(|row| include(row))
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

fn summarize_policy_mechanisms(
    harness_policy: HarnessPolicy,
    rows: &[&AutonomyResultRow],
) -> PolicyMechanismSummary {
    let mut summary = PolicyMechanismSummary {
        harness_policy,
        scoreable_episodes: 0,
        mechanisms: MechanismCounts::default(),
    };
    for row in rows
        .iter()
        .filter(|row| row.harness_policy == harness_policy && row_is_scoreable(row))
    {
        summary.scoreable_episodes += 1;
        summary.mechanisms.add(&row.metrics.mechanisms);
    }
    summary
}

fn pass_power_3(rows: &[&AutonomyResultRow]) -> PassPowerSummary {
    let mut groups: BTreeMap<(&str, &str, HarnessPolicy), Vec<&AutonomyResultRow>> =
        BTreeMap::new();
    for row in rows {
        groups
            .entry((
                row.task_id.as_str(),
                row.variant.as_str(),
                row.harness_policy,
            ))
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
        let key = (row.task_id.as_str(), row.trial, row.harness_policy);
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
    let mut tally = PairTally::default();
    for (key, recovery_completed) in &recovery {
        if let Some(brief_completed) = brief.get(key) {
            tally.record(*recovery_completed, *brief_completed);
        }
    }
    RepositoryBriefComparison {
        paired_episodes: tally.pairs(),
        recovery_wins: tally.first_wins,
        repository_brief_wins: tally.second_wins,
        both_completed: tally.both,
        neither_completed: tally.neither,
        objective_rate_delta: tally.second_minus_first_rate(),
    }
}

/// Objective outcomes of paired episodes: (first arm, second arm).
#[derive(Default)]
struct PairTally {
    first_wins: u32,
    second_wins: u32,
    both: u32,
    neither: u32,
}

impl PairTally {
    fn record(&mut self, first_completed: bool, second_completed: bool) {
        let count = match (first_completed, second_completed) {
            (true, false) => &mut self.first_wins,
            (false, true) => &mut self.second_wins,
            (true, true) => &mut self.both,
            (false, false) => &mut self.neither,
        };
        *count = count.saturating_add(1);
    }

    fn pairs(&self) -> u32 {
        self.first_wins
            .saturating_add(self.second_wins)
            .saturating_add(self.both)
            .saturating_add(self.neither)
    }

    /// Second-arm objective rate minus first-arm rate over all pairs.
    fn second_minus_first_rate(&self) -> Option<f64> {
        let pairs = self.pairs();
        let first = self.first_wins.saturating_add(self.both);
        let second = self.second_wins.saturating_add(self.both);
        (pairs > 0).then(|| (f64::from(second) - f64::from(first)) / f64::from(pairs))
    }
}

/// Every row of a run shares one binary/model/corpus provenance; only the
/// variant label and the per-episode workspace digests differ.
fn provenance_matches(rows: &[&AutonomyResultRow]) -> bool {
    let normalized = |row: &AutonomyResultRow| {
        let mut provenance = row.provenance.clone();
        provenance.variant = "autonomy_matrix".to_string();
        let mut evaluation = row.evaluation_provenance.clone();
        evaluation.workspace_instance_sha256 = None;
        evaluation.initial_tree_sha256 = None;
        (provenance, evaluation)
    };
    let Some(first) = rows.first() else {
        return true;
    };
    let first = normalized(first);
    rows.iter().all(|row| normalized(row) == first)
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
        && (row.schema_version != 1 || retained_trace_evidence_valid(row))
}

fn retained_trace_evidence_valid(row: &AutonomyResultRow) -> bool {
    !row.segments.is_empty()
        && row.segments.iter().enumerate().all(|(index, segment)| {
            segment.segment == index as u32 + 1
                && !segment.timed_out
                && segment
                    .observed_terminal
                    .as_deref()
                    .is_some_and(|terminal| !terminal.is_empty())
                && segment
                    .trace_path
                    .as_deref()
                    .is_some_and(valid_retained_trace_path)
                && valid_sha256(segment.trace_sha256.as_deref())
                && segment.trace_validation == Some(RetainedTraceValidation::StructureValidated)
        })
}

fn valid_retained_trace_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn valid_sha256(value: Option<&str>) -> bool {
    value.is_some_and(is_sha256_hex)
}

fn ratio(numerator: u32, denominator: u32) -> Option<f64> {
    (denominator > 0).then(|| f64::from(numerator) / f64::from(denominator))
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
            harness_policy: HarnessPolicy::Legacy,
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
                trace_sha256: Some("d".repeat(64)),
                trace_validation: Some(RetainedTraceValidation::StructureValidated),
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
                    params_b: 1.2,
                    ctx: 4096,
                    ..Default::default()
                },
                protocol: "unified_grammar".to_string(),
                variant: variant.to_string(),
                python_bin: "python".to_string(),
            },
            evaluation_provenance: AutonomyEvaluationProvenance {
                harness_policy: HarnessPolicy::Legacy,
                child_binary_sha256: Some("binary".to_string()),
                corpus_sha256: Some("suite".to_string()),
                query_temperature: Some("0.0".to_string()),
                ..Default::default()
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
        let observed = [&infrastructure];
        let scoreable = observed
            .iter()
            .copied()
            .filter(|row| row_is_scoreable(row))
            .collect::<Vec<_>>();
        let summary = rate_summary("test", 1, &observed, &scoreable, |_| true);
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
    fn schema_one_rows_default_to_single_legacy_coordinates() {
        let row = row("run", "A01", "ambiguity", "current", 1);
        let mut value = serde_json::to_value(row).unwrap();
        let object = value.as_object_mut().unwrap();
        object.insert("schema_version".to_string(), serde_json::json!(1));
        object.remove("harness_policy");
        object.remove("evaluation_provenance");
        object
            .get_mut("segments")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|segments| segments.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .expect("segment object")
            .remove("trace_validation");
        let decoded: AutonomyResultRow = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.harness_policy, HarnessPolicy::Legacy);
        assert_eq!(
            decoded.evaluation_provenance,
            AutonomyEvaluationProvenance::default()
        );
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
            &[decoded],
            Vec::new(),
        );
        assert!(summary.complete);
        assert!(!summary.infrastructure_clean);
        assert_eq!(summary.overall.scoreable, 0);
    }

    /// Rows written before the paired runner was retired (INT-0018) still
    /// parse. A pre-retirement single-legacy row carries exactly the
    /// information of a current row: its extra keys only held defaults.
    #[test]
    fn pre_retirement_paired_and_single_rows_still_parse() {
        use serde_json::{Value, json};

        let mut current = row("run", "H01", "long_horizon", "recovery", 1);
        current.evaluation_provenance.managed_server = Some(ManagedServerProvenance {
            engine: "llama-server".to_string(),
            listener_base_url: "http://127.0.0.1:8080/v1".to_string(),
            model: Some("models/model.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
        });
        let current_json = serde_json::to_value(&current).unwrap();

        let mut single = current_json.clone();
        single["arm"] = json!("single");
        let evaluation = &mut single["evaluation_provenance"];
        evaluation["arm"] = json!("single");
        for key in ["pair_id", "pair_slot", "pair_order"] {
            evaluation[key] = Value::Null;
        }
        for key in [
            "model_launch_argument",
            "model_sha256",
            "gpu_layers",
            "pid",
            "listener_owner_pid",
            "listener_port",
            "engine_executable",
            "engine_executable_sha256",
            "engine_version",
            "engine_argv",
        ] {
            evaluation["managed_server"][key] = Value::Null;
        }
        let decoded: AutonomyResultRow = serde_json::from_value(single).unwrap();
        assert_eq!(decoded, current);
        assert_eq!(serde_json::to_value(&decoded).unwrap(), current_json);

        let mut paired = current_json;
        paired["arm"] = json!("candidate");
        paired["harness_policy"] = json!("evidence");
        let evaluation = &mut paired["evaluation_provenance"];
        evaluation["arm"] = json!("candidate");
        evaluation["harness_policy"] = json!("evidence");
        evaluation["pair_id"] = json!("trial-001-H01-recovery");
        evaluation["pair_slot"] = json!(2);
        evaluation["pair_order"] = json!("control_candidate");
        let server = &mut evaluation["managed_server"];
        server["model_launch_argument"] = json!("models/model.gguf");
        server["model_sha256"] = json!("b".repeat(64));
        server["gpu_layers"] = json!(0);
        server["pid"] = json!(1234);
        server["listener_owner_pid"] = json!(1234);
        server["listener_port"] = json!(8080);
        server["engine_executable"] = json!("bin/llama-server");
        server["engine_executable_sha256"] = json!("e".repeat(64));
        server["engine_version"] = json!("llama-server version example");
        server["engine_argv"] = json!(["llama-server", "-m", "models/model.gguf"]);
        let decoded: AutonomyResultRow = serde_json::from_value(paired).unwrap();
        assert_eq!(decoded.harness_policy, HarnessPolicy::Evidence);
        assert_eq!(
            decoded.evaluation_provenance.harness_policy,
            HarnessPolicy::Evidence
        );
        assert_eq!(
            decoded.evaluation_provenance.managed_server,
            current.evaluation_provenance.managed_server
        );
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
        let error = crate::results::read_jsonl::<AutonomyResultRow>(
            &dir.path().join("autonomy-results.jsonl"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("line 3"));
    }

    /// The flattened mechanism counters keep every key, value and position
    /// of the historical flat row and summary documents.
    #[test]
    fn mechanism_counts_serialize_byte_identically() {
        let metrics = r#"{"turns":1,"input_tokens":2,"output_tokens":3,"tool_calls":4,"tool_errors":5,"mutations":6,"verification_passes":7,"completion_gates_passed":8,"completion_gates_blocked":9,"repetition_guard_stops":10,"no_progress_guard_stops":11,"failure_guard_stops":12,"oscillation_guard_stops":13,"truncations":14,"provider_error_stops":15,"history_compactions":16,"observations_recorded":17,"file_observations":18,"search_observations":19,"find_observations":20,"controller_blocks":21,"blind_mutation_blocks":22,"same_turn_observation_blocks":23,"stale_observation_blocks":24,"unsupported_mutation_blocks":25,"repair_inspection_blocks":26,"no_effect_blocks":27,"syntax_regression_blocks":28,"repeated_check_blocks":29,"workspace_effects_recorded":30,"workspace_effect_paths":31,"verification_checks_recorded":32,"verification_checks_passed":33,"verification_checks_failed":34,"verification_repair_attempts":35,"distinct_failed_diagnostic_fingerprints":36,"controller_checkpoints":37,"recovery_packets_injected":38,"horizon_80_turn":39,"tools_called":["read_file"]}"#;
        let parsed: AutonomyTraceMetrics = serde_json::from_str(metrics).unwrap();
        assert_eq!(parsed.mechanisms.observations_recorded, 17);
        assert_eq!(parsed.mechanisms.recovery_packets_injected, 38);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), metrics);

        let policy = r#"{"harness_policy":"evidence","scoreable_episodes":102,"observations_recorded":103,"file_observations":104,"search_observations":105,"find_observations":106,"controller_blocks":107,"blind_mutation_blocks":108,"same_turn_observation_blocks":109,"stale_observation_blocks":110,"unsupported_mutation_blocks":111,"repair_inspection_blocks":112,"no_effect_blocks":113,"syntax_regression_blocks":114,"repeated_check_blocks":115,"workspace_effects_recorded":116,"workspace_effect_paths":117,"verification_checks_recorded":118,"verification_checks_passed":119,"verification_checks_failed":120,"verification_repair_attempts":121,"distinct_failed_diagnostic_fingerprints":122,"controller_checkpoints":123,"recovery_packets_injected":124}"#;
        let parsed: PolicyMechanismSummary = serde_json::from_str(policy).unwrap();
        assert_eq!(parsed.mechanisms.observations_recorded, 103);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), policy);

        // Rows written before the mechanism counters existed still read as zero.
        let legacy: AutonomyTraceMetrics = serde_json::from_str(r#"{"turns":1,"input_tokens":2,"output_tokens":3,"tool_calls":4,"tool_errors":5,"mutations":6,"verification_passes":7,"completion_gates_passed":8,"completion_gates_blocked":9,"repetition_guard_stops":10,"no_progress_guard_stops":11,"failure_guard_stops":12,"oscillation_guard_stops":13,"truncations":14,"provider_error_stops":15,"history_compactions":16,"horizon_80_turn":null,"tools_called":[]}"#).unwrap();
        assert_eq!(legacy.mechanisms, MechanismCounts::default());
    }
}
