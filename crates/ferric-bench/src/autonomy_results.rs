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
use ferric_core::HarnessPolicy;

/// Version two adds orthogonal harness-policy/arm coordinates and paired-run
/// evidence. Every new field has a serde default so historical version-one
/// rows and summaries remain readable as single-arm legacy evidence.
pub const AUTONOMY_RESULTS_SCHEMA_VERSION: u32 = 2;
const STRICT_EVIDENCE_CONTEXT_SIZE: u32 = 8192;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyArm {
    /// Historical and current one-binary autonomy runs.
    #[default]
    Single,
    Control,
    Candidate,
}

impl AutonomyArm {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Control => "control",
            Self::Candidate => "candidate",
        }
    }
}

/// One independent evaluation coordinate. Corpus `variant` remains a separate
/// dimension; it is never overloaded with controller-policy or binary-arm
/// identity.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AutonomyEvaluationCoordinate {
    #[serde(default)]
    pub arm: AutonomyArm,
    #[serde(default)]
    pub harness_policy: HarnessPolicy,
}

impl AutonomyEvaluationCoordinate {
    pub const fn single_legacy() -> Self {
        Self {
            arm: AutonomyArm::Single,
            harness_policy: HarnessPolicy::Legacy,
        }
    }

    pub const fn paired() -> [Self; 2] {
        [
            Self {
                arm: AutonomyArm::Control,
                harness_policy: HarnessPolicy::Legacy,
            },
            Self {
                arm: AutonomyArm::Candidate,
                harness_policy: HarnessPolicy::Evidence,
            },
        ]
    }

    pub fn key(self) -> String {
        format!("{}/{}", self.arm.label(), self.harness_policy.label())
    }
}

/// Row-level provenance which was absent from schema v1. The default maps old
/// rows to their exact historical meaning: one binary using legacy policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyEvaluationProvenance {
    #[serde(default)]
    pub arm: AutonomyArm,
    #[serde(default)]
    pub harness_policy: HarnessPolicy,
    #[serde(default)]
    pub pair_id: Option<String>,
    #[serde(default)]
    pub pair_slot: Option<u8>,
    #[serde(default)]
    pub pair_order: Option<String>,
    /// Digest of the canonical temporary-workspace identity. This establishes
    /// pair independence without retaining a machine-specific path.
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedServerProvenance {
    pub engine: String,
    pub listener_base_url: String,
    /// Canonical local model artifact identity established during live
    /// validation.
    pub model: Option<String>,
    /// Exact `-m` value observed in the live engine argv.
    #[serde(default)]
    pub model_launch_argument: Option<String>,
    #[serde(default)]
    pub model_sha256: Option<String>,
    pub context_size: Option<u32>,
    pub sampling_seed: Option<i64>,
    pub parallel_slots: Option<u32>,
    #[serde(default)]
    pub gpu_layers: Option<u32>,
    /// Live process identity, not merely text copied from the runfile.
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub listener_owner_pid: Option<u32>,
    #[serde(default)]
    pub listener_port: Option<u16>,
    #[serde(default)]
    pub engine_executable: Option<String>,
    #[serde(default)]
    pub engine_executable_sha256: Option<String>,
    #[serde(default)]
    pub engine_version: Option<String>,
    #[serde(default)]
    pub engine_argv: Option<Vec<String>>,
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
    #[serde(default)]
    pub observations_recorded: u32,
    #[serde(default)]
    pub file_observations: u32,
    #[serde(default)]
    pub search_observations: u32,
    #[serde(default)]
    pub find_observations: u32,
    #[serde(default)]
    pub controller_blocks: u32,
    #[serde(default)]
    pub blind_mutation_blocks: u32,
    #[serde(default)]
    pub same_turn_observation_blocks: u32,
    #[serde(default)]
    pub stale_observation_blocks: u32,
    #[serde(default)]
    pub unsupported_mutation_blocks: u32,
    #[serde(default)]
    pub repair_inspection_blocks: u32,
    #[serde(default)]
    pub no_effect_blocks: u32,
    #[serde(default)]
    pub syntax_regression_blocks: u32,
    #[serde(default)]
    pub repeated_check_blocks: u32,
    #[serde(default)]
    pub workspace_effects_recorded: u32,
    #[serde(default)]
    pub workspace_effect_paths: u32,
    #[serde(default)]
    pub verification_checks_recorded: u32,
    #[serde(default)]
    pub verification_checks_passed: u32,
    #[serde(default)]
    pub verification_checks_failed: u32,
    #[serde(default)]
    pub verification_repair_attempts: u32,
    #[serde(default)]
    pub distinct_failed_diagnostic_fingerprints: u32,
    #[serde(default)]
    pub controller_checkpoints: u32,
    #[serde(default)]
    pub recovery_packets_injected: u32,
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
    #[serde(default)]
    pub arm: AutonomyArm,
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
    /// Sum of per-episode distinct fingerprint counts. Digests are deliberately
    /// not copied into aggregate output.
    pub distinct_failed_diagnostic_fingerprints: u32,
    pub controller_checkpoints: u32,
    pub recovery_packets_injected: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairedObjectiveSummary {
    /// Expected task×variant×trial pairs, including excluded infrastructure
    /// or incomplete pairs.
    pub expected_pairs: u32,
    /// Pairs with both arms, distinct fresh workspaces, valid coordinates, and
    /// no infrastructure failure on either row.
    pub eligible_pairs: u32,
    pub excluded_pairs: u32,
    pub control_wins: u32,
    pub candidate_wins: u32,
    pub both_completed: u32,
    pub neither_completed: u32,
    /// Candidate objective rate minus control objective rate on eligible pairs
    /// only. An excluded control row is never counted as a model loss.
    pub objective_rate_delta: Option<f64>,
    /// Task IDs with at least one qualifying task/variant coordinate.
    #[serde(default)]
    pub candidate_tasks_at_least_two_of_three: Vec<String>,
    /// Task/variant coordinates whose first three eligible paired trials
    /// include at least two candidate objective completions.
    #[serde(default)]
    pub candidate_task_variants_at_least_two_of_three: Vec<String>,
    pub task_evidence_threshold_met: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoordinateProvenanceSummary {
    pub coordinate: AutonomyEvaluationCoordinate,
    pub provenance: RunProvenance,
    #[serde(default)]
    pub evaluation_provenance: AutonomyEvaluationProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyRunIssue {
    pub task_id: Option<String>,
    pub variant: Option<String>,
    #[serde(default)]
    pub arm: Option<AutonomyArm>,
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
    #[serde(default)]
    pub expected_coordinates: Vec<AutonomyEvaluationCoordinate>,
    pub expected_rows: u32,
    pub observed_rows: u32,
    pub complete: bool,
    pub infrastructure_clean: bool,
    pub internal_baseline_only: bool,
    pub provenance: Option<RunProvenance>,
    #[serde(default)]
    pub provenance_by_coordinate: Vec<CoordinateProvenanceSummary>,
    pub server_states: Vec<String>,
    pub issues: Vec<AutonomyRunIssue>,
    pub overall: AutonomyRateSummary,
    pub by_task: Vec<AutonomyRateSummary>,
    pub by_task_variant: Vec<AutonomyRateSummary>,
    pub by_category: Vec<AutonomyRateSummary>,
    pub by_variant: Vec<AutonomyRateSummary>,
    #[serde(default)]
    pub by_harness_policy: Vec<AutonomyRateSummary>,
    #[serde(default)]
    pub by_arm: Vec<AutonomyRateSummary>,
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
    #[serde(default)]
    pub paired_objective: Option<PairedObjectiveSummary>,
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
    summarize_autonomy_run_with_coordinates(
        run_id,
        suite_id,
        suite_schema_version,
        suite_sha256,
        started_at_unix_ms,
        finished_at_unix_ms,
        trials_requested,
        expected_tasks,
        expected_task_categories,
        expected_variants,
        &[AutonomyEvaluationCoordinate::single_legacy()],
        rows,
        issues,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn summarize_autonomy_run_with_coordinates(
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
    expected_coordinates: &[AutonomyEvaluationCoordinate],
    rows: &[AutonomyResultRow],
    issues: Vec<AutonomyRunIssue>,
) -> AutonomyRunSummary {
    let run_rows: Vec<&AutonomyResultRow> =
        rows.iter().filter(|row| row.run_id == run_id).collect();
    let coordinate_count = expected_coordinates.len() as u32;
    let expected_rows = trials_requested
        .saturating_mul(expected_tasks.len() as u32)
        .saturating_mul(expected_variants.len() as u32)
        .saturating_mul(coordinate_count);
    let observed_coordinates: BTreeSet<_> = run_rows
        .iter()
        .map(|row| {
            (
                row.task_id.clone(),
                row.variant.clone(),
                row.trial,
                row.arm,
                row.harness_policy,
            )
        })
        .collect();
    let expected_row_coordinates: BTreeSet<_> = expected_tasks
        .iter()
        .flat_map(|task| {
            expected_variants.iter().flat_map(move |variant| {
                (1..=trials_requested).flat_map(move |trial| {
                    expected_coordinates.iter().map(move |coordinate| {
                        (
                            task.clone(),
                            variant.clone(),
                            trial,
                            coordinate.arm,
                            coordinate.harness_policy,
                        )
                    })
                })
            })
        })
        .collect();
    let expected_task_set: BTreeSet<_> = expected_tasks.iter().cloned().collect();
    let expected_variant_set: BTreeSet<_> = expected_variants.iter().cloned().collect();
    let expected_coordinate_set: BTreeSet<_> = expected_coordinates.iter().copied().collect();
    let expected_dimensions_unique = trials_requested > 0
        && !expected_task_set.is_empty()
        && !expected_variant_set.is_empty()
        && !expected_coordinate_set.is_empty()
        && expected_task_set.len() == expected_tasks.len()
        && expected_variant_set.len() == expected_variants.len()
        && expected_coordinate_set.len() == expected_coordinates.len();
    let category_contract_matches = expected_task_categories.len() == expected_task_set.len()
        && expected_task_categories
            .keys()
            .all(|task| expected_task_set.contains(task));
    let row_contract_matches = run_rows.iter().all(|row| {
        let schema_coordinate_matches = if row.schema_version == AUTONOMY_RESULTS_SCHEMA_VERSION {
            row.evaluation_provenance.arm == row.arm
                && row.evaluation_provenance.harness_policy == row.harness_policy
                && row.evaluation_provenance.child_binary_sha256 == row.provenance.binary.sha256
                && row.evaluation_provenance.corpus_sha256.as_deref() == Some(suite_sha256)
                && row.evaluation_provenance.model_sha256 == row.provenance.model.sha256
                && row.evaluation_provenance.query_temperature.as_deref() == Some("0.0")
        } else {
            row.schema_version == 1
                && coordinate_count == 1
                && row.arm == AutonomyArm::Single
                && row.harness_policy == HarnessPolicy::Legacy
                && row.evaluation_provenance == AutonomyEvaluationProvenance::default()
        };
        schema_coordinate_matches
            && row.suite_id == suite_id
            && row.suite_schema_version == suite_schema_version
            && row.suite_sha256 == suite_sha256
            && expected_task_categories.get(&row.task_id) == Some(&row.category)
            && row.provenance.variant == row.variant
            && (!strict_evidence_coordinate(row) || strict_persisted_row_valid(row))
    });
    let provenance_matches = coordinate_provenance_matches(&run_rows);
    let paired_metadata_matches = paired_metadata_matches(&run_rows, coordinate_count as usize);
    let complete = run_rows.len() as u32 == expected_rows
        && observed_coordinates == expected_row_coordinates
        && category_contract_matches
        && row_contract_matches
        && provenance_matches
        && paired_metadata_matches
        && expected_dimensions_unique;
    let infrastructure_clean =
        issues.is_empty() && run_rows.iter().all(|row| row_is_scoreable(row));
    let provenance = (coordinate_count == 1)
        .then(|| {
            run_rows.first().map(|row| {
                let mut provenance = row.provenance.clone();
                provenance.variant = "autonomy_matrix".to_string();
                provenance
            })
        })
        .flatten();
    let provenance_by_coordinate = coordinate_provenance_summaries(&run_rows);
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
    let coordinate_keys: BTreeSet<_> = expected_row_coordinates
        .iter()
        .map(|coordinate| (coordinate.3, coordinate.4))
        .collect();
    let policy_keys: BTreeSet<_> = coordinate_keys.iter().map(|(_, policy)| *policy).collect();
    let arm_keys: BTreeSet<_> = coordinate_keys.iter().map(|(arm, _)| *arm).collect();
    let category_keys: BTreeSet<String> = expected_task_categories.values().cloned().collect();
    // A paired experiment has one statistical unit: the complete control /
    // candidate pair. Derive that eligibility once and use the resulting rows
    // for every model-attributable statistic below. Otherwise a clean-looking
    // survivor of a missing or infrastructure-dirty arm could still leak into
    // the overall/policy/mechanism rates even though the paired delta excludes
    // it.
    let eligible_pairs = (coordinate_count == 2).then(|| eligible_paired_rows(&run_rows));
    let scoreable_rows: Vec<_> = match &eligible_pairs {
        Some(pairs) => pairs
            .iter()
            .flat_map(|(control, candidate)| [*control, *candidate])
            .collect(),
        None => run_rows
            .iter()
            .copied()
            .filter(|row| row_is_scoreable(row))
            .collect(),
    };

    let overall = rate_summary("all", expected_rows, &run_rows, &scoreable_rows, |_| true);
    let by_task = task_keys
        .iter()
        .map(|key| {
            rate_summary(
                key,
                trials_requested
                    .saturating_mul(variant_keys.len() as u32)
                    .saturating_mul(coordinate_count),
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
                    trials_requested.saturating_mul(coordinate_count),
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
                    .saturating_mul(tasks_in_category)
                    .saturating_mul(coordinate_count),
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
                trials_requested
                    .saturating_mul(task_keys.len() as u32)
                    .saturating_mul(coordinate_count),
                &run_rows,
                &scoreable_rows,
                |row| &row.variant == key,
            )
        })
        .collect();
    let by_harness_policy = policy_keys
        .iter()
        .map(|policy| {
            let policy_coordinate_count = expected_coordinates
                .iter()
                .filter(|coordinate| coordinate.harness_policy == *policy)
                .count() as u32;
            rate_summary(
                policy.label(),
                trials_requested
                    .saturating_mul(task_keys.len() as u32)
                    .saturating_mul(variant_keys.len() as u32)
                    .saturating_mul(policy_coordinate_count),
                &run_rows,
                &scoreable_rows,
                |row| row.harness_policy == *policy,
            )
        })
        .collect();
    let by_arm = arm_keys
        .iter()
        .map(|arm| {
            let arm_coordinate_count = expected_coordinates
                .iter()
                .filter(|coordinate| coordinate.arm == *arm)
                .count() as u32;
            rate_summary(
                arm.label(),
                trials_requested
                    .saturating_mul(task_keys.len() as u32)
                    .saturating_mul(variant_keys.len() as u32)
                    .saturating_mul(arm_coordinate_count),
                &run_rows,
                &scoreable_rows,
                |row| row.arm == *arm,
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
        (task_keys.len() as u32)
            .saturating_mul(variant_keys.len() as u32)
            .saturating_mul(coordinate_count),
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
    let paired_objective = eligible_pairs.as_ref().map(|pairs| {
        summarize_paired_objective(
            trials_requested
                .saturating_mul(task_keys.len() as u32)
                .saturating_mul(variant_keys.len() as u32),
            pairs,
        )
    });

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
        expected_coordinates: expected_coordinates.to_vec(),
        expected_rows,
        observed_rows: run_rows.len() as u32,
        complete,
        infrastructure_clean,
        internal_baseline_only: true,
        provenance,
        provenance_by_coordinate,
        server_states: server_states.into_iter().collect(),
        issues,
        overall,
        by_task,
        by_task_variant,
        by_category,
        by_variant,
        by_harness_policy,
        by_arm,
        by_tool,
        by_policy_mechanisms,
        clarification,
        recovery,
        resolved_at_1,
        pass_power_3: aggregate_pass_power_3,
        pass_power_3_by_policy,
        repository_brief_ab,
        repository_brief_ab_by_policy,
        paired_objective,
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
    let rows = rows
        .iter()
        .copied()
        .filter(|row| row.harness_policy == harness_policy && row_is_scoreable(row))
        .collect::<Vec<_>>();
    let sum = |value: fn(&AutonomyTraceMetrics) -> u32| {
        rows.iter().fold(0_u32, |total, row| {
            total.saturating_add(value(&row.metrics))
        })
    };
    PolicyMechanismSummary {
        harness_policy,
        scoreable_episodes: rows.len() as u32,
        observations_recorded: sum(|metrics| metrics.observations_recorded),
        file_observations: sum(|metrics| metrics.file_observations),
        search_observations: sum(|metrics| metrics.search_observations),
        find_observations: sum(|metrics| metrics.find_observations),
        controller_blocks: sum(|metrics| metrics.controller_blocks),
        blind_mutation_blocks: sum(|metrics| metrics.blind_mutation_blocks),
        same_turn_observation_blocks: sum(|metrics| metrics.same_turn_observation_blocks),
        stale_observation_blocks: sum(|metrics| metrics.stale_observation_blocks),
        unsupported_mutation_blocks: sum(|metrics| metrics.unsupported_mutation_blocks),
        repair_inspection_blocks: sum(|metrics| metrics.repair_inspection_blocks),
        no_effect_blocks: sum(|metrics| metrics.no_effect_blocks),
        syntax_regression_blocks: sum(|metrics| metrics.syntax_regression_blocks),
        repeated_check_blocks: sum(|metrics| metrics.repeated_check_blocks),
        workspace_effects_recorded: sum(|metrics| metrics.workspace_effects_recorded),
        workspace_effect_paths: sum(|metrics| metrics.workspace_effect_paths),
        verification_checks_recorded: sum(|metrics| metrics.verification_checks_recorded),
        verification_checks_passed: sum(|metrics| metrics.verification_checks_passed),
        verification_checks_failed: sum(|metrics| metrics.verification_checks_failed),
        verification_repair_attempts: sum(|metrics| metrics.verification_repair_attempts),
        distinct_failed_diagnostic_fingerprints: sum(|metrics| {
            metrics.distinct_failed_diagnostic_fingerprints
        }),
        controller_checkpoints: sum(|metrics| metrics.controller_checkpoints),
        recovery_packets_injected: sum(|metrics| metrics.recovery_packets_injected),
    }
}

fn pass_power_3(rows: &[&AutonomyResultRow]) -> PassPowerSummary {
    let mut groups: BTreeMap<(&str, &str, HarnessPolicy, AutonomyArm), Vec<&AutonomyResultRow>> =
        BTreeMap::new();
    for row in rows {
        groups
            .entry((
                row.task_id.as_str(),
                row.variant.as_str(),
                row.harness_policy,
                row.arm,
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
        let key = (row.task_id.as_str(), row.trial, row.harness_policy, row.arm);
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

fn coordinate_provenance_matches(rows: &[&AutonomyResultRow]) -> bool {
    let Some(first) = rows.first() else {
        return true;
    };
    if rows.iter().any(|row| {
        row.provenance.model != first.provenance.model
            || row.provenance.protocol != first.provenance.protocol
            || row.provenance.python_bin != first.provenance.python_bin
    }) {
        return false;
    }

    let mut exemplars: BTreeMap<
        (AutonomyArm, HarnessPolicy),
        (RunProvenance, AutonomyEvaluationProvenance),
    > = BTreeMap::new();
    for row in rows {
        let mut normalized = row.provenance.clone();
        normalized.variant = "autonomy_matrix".to_string();
        let mut evaluation = row.evaluation_provenance.clone();
        evaluation.pair_id = None;
        evaluation.pair_slot = None;
        evaluation.pair_order = None;
        evaluation.workspace_instance_sha256 = None;
        evaluation.initial_tree_sha256 = None;
        match exemplars.entry((row.arm, row.harness_policy)) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((normalized, evaluation));
            }
            std::collections::btree_map::Entry::Occupied(entry)
                if entry.get() != &(normalized, evaluation) =>
            {
                return false;
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
    }
    true
}

fn coordinate_provenance_summaries(
    rows: &[&AutonomyResultRow],
) -> Vec<CoordinateProvenanceSummary> {
    let mut exemplars = BTreeMap::new();
    for row in rows {
        let mut provenance = row.provenance.clone();
        provenance.variant = "autonomy_matrix".to_string();
        let mut evaluation_provenance = row.evaluation_provenance.clone();
        evaluation_provenance.pair_id = None;
        evaluation_provenance.pair_slot = None;
        evaluation_provenance.pair_order = None;
        evaluation_provenance.workspace_instance_sha256 = None;
        evaluation_provenance.initial_tree_sha256 = None;
        exemplars
            .entry((row.arm, row.harness_policy))
            .or_insert((provenance, evaluation_provenance));
    }
    exemplars
        .into_iter()
        .map(
            |((arm, harness_policy), (provenance, evaluation_provenance))| {
                CoordinateProvenanceSummary {
                    coordinate: AutonomyEvaluationCoordinate {
                        arm,
                        harness_policy,
                    },
                    provenance,
                    evaluation_provenance,
                }
            },
        )
        .collect()
}

fn paired_metadata_matches(rows: &[&AutonomyResultRow], coordinate_count: usize) -> bool {
    if coordinate_count == 1 {
        return rows.iter().all(|row| {
            row.evaluation_provenance.pair_id.is_none()
                && row.evaluation_provenance.pair_slot.is_none()
                && row.evaluation_provenance.pair_order.is_none()
        });
    }
    if coordinate_count != 2 {
        return false;
    }
    let mut pairs: BTreeMap<&str, Vec<&AutonomyResultRow>> = BTreeMap::new();
    for row in rows {
        let Some(pair_id) = row.evaluation_provenance.pair_id.as_deref() else {
            return false;
        };
        pairs.entry(pair_id).or_default().push(row);
    }
    pairs.values().all(|pair| paired_rows(pair).is_some())
}

type EligiblePair<'a> = (&'a AutonomyResultRow, &'a AutonomyResultRow);

fn eligible_paired_rows<'a>(rows: &[&'a AutonomyResultRow]) -> Vec<EligiblePair<'a>> {
    let mut groups: BTreeMap<&str, Vec<&AutonomyResultRow>> = BTreeMap::new();
    for row in rows {
        if let Some(pair_id) = row.evaluation_provenance.pair_id.as_deref() {
            groups.entry(pair_id).or_default().push(row);
        }
    }
    groups
        .values()
        .filter_map(|group| {
            let (control, candidate) = paired_rows(group)?;
            (row_is_scoreable(control) && row_is_scoreable(candidate))
                .then_some((control, candidate))
        })
        .collect()
}

fn paired_rows<'a>(
    rows: &[&'a AutonomyResultRow],
) -> Option<(&'a AutonomyResultRow, &'a AutonomyResultRow)> {
    if rows.len() != 2 {
        return None;
    }
    let control = rows.iter().copied().find(|row| {
        row.arm == AutonomyArm::Control && row.harness_policy == HarnessPolicy::Legacy
    })?;
    let candidate = rows.iter().copied().find(|row| {
        row.arm == AutonomyArm::Candidate && row.harness_policy == HarnessPolicy::Evidence
    })?;
    let canonical_pair_id = format!(
        "trial-{:03}-{}-{}",
        control.trial, control.task_id, control.variant
    );
    if control.evaluation_provenance.pair_id.as_deref() != Some(canonical_pair_id.as_str())
        || candidate.evaluation_provenance.pair_id.as_deref() != Some(canonical_pair_id.as_str())
    {
        return None;
    }
    let order = control.evaluation_provenance.pair_order.as_deref()?;
    if candidate.evaluation_provenance.pair_order.as_deref() != Some(order) {
        return None;
    }
    let slots_match = match order {
        "control_candidate" => {
            control.evaluation_provenance.pair_slot == Some(1)
                && candidate.evaluation_provenance.pair_slot == Some(2)
        }
        "candidate_control" => {
            candidate.evaluation_provenance.pair_slot == Some(1)
                && control.evaluation_provenance.pair_slot == Some(2)
        }
        _ => false,
    };
    if !slots_match
        || control.task_id != candidate.task_id
        || control.variant != candidate.variant
        || control.trial != candidate.trial
        || !valid_sha256(
            control
                .evaluation_provenance
                .workspace_instance_sha256
                .as_deref(),
        )
        || !valid_sha256(
            candidate
                .evaluation_provenance
                .workspace_instance_sha256
                .as_deref(),
        )
        || control.evaluation_provenance.workspace_instance_sha256
            == candidate.evaluation_provenance.workspace_instance_sha256
        || !valid_sha256(control.evaluation_provenance.initial_tree_sha256.as_deref())
        || !valid_sha256(
            candidate
                .evaluation_provenance
                .initial_tree_sha256
                .as_deref(),
        )
        || control.evaluation_provenance.initial_tree_sha256
            != candidate.evaluation_provenance.initial_tree_sha256
        || control.evaluation_provenance.child_binary_sha256.is_none()
        || candidate
            .evaluation_provenance
            .child_binary_sha256
            .is_none()
        || control.evaluation_provenance.child_binary_sha256
            == candidate.evaluation_provenance.child_binary_sha256
        || control.evaluation_provenance.corpus_sha256
            != candidate.evaluation_provenance.corpus_sha256
        || control.evaluation_provenance.model_sha256.is_none()
        || control.evaluation_provenance.model_sha256
            != candidate.evaluation_provenance.model_sha256
        || control.evaluation_provenance.query_temperature.as_deref() != Some("0.0")
        || candidate.evaluation_provenance.query_temperature.as_deref() != Some("0.0")
        || control.evaluation_provenance.managed_server.is_none()
        || control.evaluation_provenance.managed_server
            != candidate.evaluation_provenance.managed_server
        || control
            .evaluation_provenance
            .managed_server
            .as_ref()
            .and_then(|server| server.model_sha256.as_ref())
            != control.evaluation_provenance.model_sha256.as_ref()
        || candidate
            .evaluation_provenance
            .managed_server
            .as_ref()
            .and_then(|server| server.model_sha256.as_ref())
            != candidate.evaluation_provenance.model_sha256.as_ref()
    {
        return None;
    }
    Some((control, candidate))
}

fn summarize_paired_objective(
    expected_pairs: u32,
    pairs: &[EligiblePair<'_>],
) -> PairedObjectiveSummary {
    let mut control_wins = 0_u32;
    let mut candidate_wins = 0_u32;
    let mut both_completed = 0_u32;
    let mut neither_completed = 0_u32;
    let mut task_trials: BTreeMap<(&str, &str), BTreeMap<u32, bool>> = BTreeMap::new();
    for &(control, candidate) in pairs {
        match (control.objective_completed, candidate.objective_completed) {
            (true, false) => control_wins = control_wins.saturating_add(1),
            (false, true) => candidate_wins = candidate_wins.saturating_add(1),
            (true, true) => both_completed = both_completed.saturating_add(1),
            (false, false) => neither_completed = neither_completed.saturating_add(1),
        }
        task_trials
            .entry((candidate.task_id.as_str(), candidate.variant.as_str()))
            .or_default()
            .insert(candidate.trial, candidate.objective_completed);
    }
    let eligible_pairs = control_wins
        .saturating_add(candidate_wins)
        .saturating_add(both_completed)
        .saturating_add(neither_completed);
    let control_completed = control_wins.saturating_add(both_completed);
    let candidate_completed = candidate_wins.saturating_add(both_completed);
    let candidate_task_variants_at_least_two_of_three = task_trials
        .into_iter()
        .filter_map(|((task, variant), trials)| {
            let first_three = (1..=3)
                .map(|trial| trials.get(&trial).copied())
                .collect::<Option<Vec<_>>>()?;
            (first_three
                .into_iter()
                .filter(|completed| *completed)
                .count()
                >= 2)
                .then(|| format!("{task}/{variant}"))
        })
        .collect::<Vec<_>>();
    let candidate_tasks_at_least_two_of_three = candidate_task_variants_at_least_two_of_three
        .iter()
        .filter_map(|coordinate| coordinate.split_once('/').map(|(task, _)| task.to_string()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    PairedObjectiveSummary {
        expected_pairs,
        eligible_pairs,
        excluded_pairs: expected_pairs.saturating_sub(eligible_pairs),
        control_wins,
        candidate_wins,
        both_completed,
        neither_completed,
        objective_rate_delta: (eligible_pairs > 0).then(|| {
            (f64::from(candidate_completed) - f64::from(control_completed))
                / f64::from(eligible_pairs)
        }),
        task_evidence_threshold_met: !candidate_tasks_at_least_two_of_three.is_empty(),
        candidate_tasks_at_least_two_of_three,
        candidate_task_variants_at_least_two_of_three,
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
        && if row.schema_version == 1 {
            retained_trace_evidence_valid(row)
        } else if strict_evidence_coordinate(row) {
            strict_persisted_row_valid(row)
        } else {
            true
        }
}

fn strict_evidence_coordinate(row: &AutonomyResultRow) -> bool {
    row.arm != AutonomyArm::Single || row.harness_policy == HarnessPolicy::Evidence
}

fn strict_persisted_row_valid(row: &AutonomyResultRow) -> bool {
    row.schema_version == AUTONOMY_RESULTS_SCHEMA_VERSION
        && strict_retained_trace_evidence_valid(row)
        && valid_sha256(
            row.evaluation_provenance
                .workspace_instance_sha256
                .as_deref(),
        )
        && valid_sha256(row.evaluation_provenance.initial_tree_sha256.as_deref())
        && valid_sha256(row.provenance.binary.sha256.as_deref())
        && row.evaluation_provenance.child_binary_sha256 == row.provenance.binary.sha256
        && valid_sha256(row.evaluation_provenance.corpus_sha256.as_deref())
        && row.evaluation_provenance.corpus_sha256.as_deref() == Some(row.suite_sha256.as_str())
        && valid_sha256(row.provenance.model.sha256.as_deref())
        && row.evaluation_provenance.model_sha256 == row.provenance.model.sha256
        && row
            .evaluation_provenance
            .managed_server
            .as_ref()
            .is_some_and(|server| strict_managed_server_valid(server, row))
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

fn strict_retained_trace_evidence_valid(row: &AutonomyResultRow) -> bool {
    if row.segments.is_empty() {
        return false;
    }
    let mut saw_timeout = false;
    for (index, segment) in row.segments.iter().enumerate() {
        let retained_evidence = segment.segment == index as u32 + 1
            && segment
                .trace_path
                .as_deref()
                .is_some_and(valid_retained_trace_path)
            && valid_sha256(segment.trace_sha256.as_deref())
            && segment.trace_validation == Some(RetainedTraceValidation::StructureValidated);
        if !retained_evidence {
            return false;
        }
        if segment.timed_out {
            if saw_timeout || index + 1 != row.segments.len() || segment.observed_terminal.is_some()
            {
                return false;
            }
            saw_timeout = true;
            if segment.exit_code.is_some() {
                return false;
            }
        } else if segment
            .observed_terminal
            .as_deref()
            .is_none_or(str::is_empty)
        {
            return false;
        }
    }
    !saw_timeout
        || (!row.contract_passed
            && !row.objective_completed
            && row.final_terminal.is_none()
            && row.command_checks.is_empty())
}

fn valid_retained_trace_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn strict_managed_server_valid(server: &ManagedServerProvenance, row: &AutonomyResultRow) -> bool {
    let exact_loopback_endpoint = server
        .listener_port
        .is_some_and(|port| server.listener_base_url == format!("http://127.0.0.1:{port}/v1"));
    if server.engine != "llama-server"
        || !exact_loopback_endpoint
        || server.listener_base_url.trim().is_empty()
        || row
            .provenance
            .model
            .api_base
            .as_deref()
            .is_none_or(str::is_empty)
        || normalized_endpoint(&server.listener_base_url)
            != normalized_endpoint(row.provenance.model.api_base.as_deref().unwrap_or_default())
        || server.model.as_deref().is_none_or(str::is_empty)
        || server
            .model
            .as_deref()
            .is_none_or(|model| !canonical_absolute_path_text(model))
        || server
            .model_launch_argument
            .as_deref()
            .is_none_or(str::is_empty)
        || row
            .provenance
            .model
            .model
            .as_deref()
            .is_none_or(str::is_empty)
        || server.model != row.provenance.model.model
        || server.context_size != Some(STRICT_EVIDENCE_CONTEXT_SIZE)
        || row.provenance.model.ctx != STRICT_EVIDENCE_CONTEXT_SIZE
        || server.sampling_seed.is_none_or(|seed| seed < 0)
        || server.parallel_slots != Some(1)
        || server.gpu_layers != Some(0)
        || server.pid.is_none_or(|pid| pid == 0)
        || server.listener_owner_pid != server.pid
        || server.listener_port.is_none_or(|port| port == 0)
        || server
            .engine_executable
            .as_deref()
            .is_none_or(str::is_empty)
        || server
            .engine_executable
            .as_deref()
            .is_none_or(|path| !canonical_absolute_path_text(path))
        || !valid_sha256(server.engine_executable_sha256.as_deref())
        || server.engine_version.as_deref().is_none_or(str::is_empty)
        || server.engine_argv.as_ref().is_none_or(Vec::is_empty)
        || server.model_sha256 != row.provenance.model.sha256
        || !valid_sha256(server.model_sha256.as_deref())
    {
        return false;
    }

    let argv = server.engine_argv.as_deref().unwrap_or_default();
    let model_argument = server.model_launch_argument.as_deref().unwrap_or_default();
    let context = row.provenance.model.ctx.to_string();
    let seed = server.sampling_seed.unwrap_or_default().to_string();
    let port = server.listener_port.unwrap_or_default().to_string();
    executable_is_llama_server(server.engine_executable.as_deref().unwrap_or_default())
        && argv
            .first()
            .is_some_and(|program| executable_is_llama_server(program))
        && argv_value(argv, &["-m", "--model"]) == Some(model_argument)
        && argv_value(argv, &["-c", "--ctx-size"]) == Some(context.as_str())
        && argv_value(argv, &["--seed"]) == Some(seed.as_str())
        && argv_value(argv, &["--parallel"]) == Some("1")
        && argv_value(argv, &["-ngl", "--gpu-layers"]) == Some("0")
        && argv_value(argv, &["--host"]) == Some("127.0.0.1")
        && argv_value(argv, &["--port"]) == Some(port.as_str())
}

fn argv_value<'a>(argv: &'a [String], flags: &[&str]) -> Option<&'a str> {
    argv.windows(2)
        .find(|window| flags.contains(&window[0].as_str()))
        .map(|window| window[1].as_str())
}

fn canonical_absolute_path_text(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        && path.components().all(|component| {
            !matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
}

fn executable_is_llama_server(value: &str) -> bool {
    Path::new(value)
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("llama-server"))
}

fn normalized_endpoint(value: &str) -> &str {
    value.trim_end_matches('/')
}

fn valid_sha256(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
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
            arm: AutonomyArm::Single,
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
            evaluation_provenance: AutonomyEvaluationProvenance {
                arm: AutonomyArm::Single,
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

    fn managed_server() -> ManagedServerProvenance {
        let canonical_model = std::env::current_dir()
            .unwrap()
            .join("models/model.gguf")
            .display()
            .to_string();
        ManagedServerProvenance {
            engine: "llama-server".to_string(),
            listener_base_url: "http://127.0.0.1:8080/v1".to_string(),
            model: Some(canonical_model),
            model_launch_argument: Some("models/model.gguf".to_string()),
            model_sha256: Some("b".repeat(64)),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            gpu_layers: Some(0),
            pid: Some(1234),
            listener_owner_pid: Some(1234),
            listener_port: Some(8080),
            engine_executable: Some(
                std::env::current_dir()
                    .unwrap()
                    .join("bin/llama-server")
                    .display()
                    .to_string(),
            ),
            engine_executable_sha256: Some("e".repeat(64)),
            engine_version: Some("llama-server version example".to_string()),
            engine_argv: Some(vec![
                "llama-server".to_string(),
                "-m".to_string(),
                "models/model.gguf".to_string(),
                "-c".to_string(),
                "8192".to_string(),
                "--seed".to_string(),
                "42".to_string(),
                "--parallel".to_string(),
                "1".to_string(),
                "-ngl".to_string(),
                "0".to_string(),
                "--host".to_string(),
                "127.0.0.1".to_string(),
                "--port".to_string(),
                "8080".to_string(),
            ]),
        }
    }

    fn paired_row(
        task: &str,
        variant: &str,
        trial: u32,
        arm: AutonomyArm,
        completed: bool,
    ) -> AutonomyResultRow {
        let mut row = row("run", task, "long_horizon", variant, trial);
        let (harness_policy, binary_sha, workspace, slot) = match arm {
            AutonomyArm::Control => (
                HarnessPolicy::Legacy,
                "c".repeat(64),
                "workspace-control",
                1,
            ),
            AutonomyArm::Candidate => (
                HarnessPolicy::Evidence,
                "a".repeat(64),
                "workspace-candidate",
                2,
            ),
            AutonomyArm::Single => panic!("paired fixture requires a paired arm"),
        };
        row.arm = arm;
        row.harness_policy = harness_policy;
        row.objective_completed = completed;
        row.suite_sha256 = "5".repeat(64);
        row.provenance.binary.path = format!("{binary_sha}-ferric");
        row.provenance.binary.sha256 = Some(binary_sha.clone());
        row.provenance.model.backend = "openai".to_string();
        row.provenance.model.model = managed_server().model;
        row.provenance.model.api_base = Some("http://127.0.0.1:8080/v1".to_string());
        row.provenance.model.ctx = 8192;
        row.provenance.model.sha256 = Some("b".repeat(64));
        row.evaluation_provenance = AutonomyEvaluationProvenance {
            arm,
            harness_policy,
            pair_id: Some(format!("trial-{trial:03}-{task}-{variant}")),
            pair_slot: Some(slot),
            pair_order: Some("control_candidate".to_string()),
            workspace_instance_sha256: Some(crate::sha256_bytes(
                format!("{workspace}-{trial}-{variant}").as_bytes(),
            )),
            initial_tree_sha256: Some("9".repeat(64)),
            child_binary_sha256: Some(binary_sha),
            corpus_sha256: Some("5".repeat(64)),
            model_sha256: Some("b".repeat(64)),
            query_temperature: Some("0.0".to_string()),
            managed_server: Some(managed_server()),
        };
        row
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
    fn paired_summary_is_coordinate_complete_and_uses_only_eligible_pairs() {
        let mut rows = Vec::new();
        for (trial, control, candidate) in [(1, false, true), (2, false, true), (3, true, false)] {
            rows.push(paired_row(
                "H01",
                "recovery",
                trial,
                AutonomyArm::Control,
                control,
            ));
            rows.push(paired_row(
                "H01",
                "recovery",
                trial,
                AutonomyArm::Candidate,
                candidate,
            ));
        }
        for row in rows
            .iter_mut()
            .filter(|row| row.harness_policy == HarnessPolicy::Evidence)
        {
            row.metrics.observations_recorded = 1;
            row.metrics.file_observations = 1;
        }
        let summary = summarize_autonomy_run_with_coordinates(
            "run",
            "autonomy-v1",
            1,
            &"5".repeat(64),
            1,
            2,
            3,
            &["H01".to_string()],
            &categories(&[("H01", "long_horizon")]),
            &["recovery".to_string()],
            &AutonomyEvaluationCoordinate::paired(),
            &rows,
            Vec::new(),
        );
        assert!(summary.complete);
        assert_eq!(summary.by_harness_policy.len(), 2);
        assert_eq!(summary.provenance_by_coordinate.len(), 2);
        assert_eq!(summary.by_policy_mechanisms.len(), 2);
        let evidence_mechanisms = summary
            .by_policy_mechanisms
            .iter()
            .find(|summary| summary.harness_policy == HarnessPolicy::Evidence)
            .unwrap();
        assert_eq!(evidence_mechanisms.scoreable_episodes, 3);
        assert_eq!(evidence_mechanisms.observations_recorded, 3);
        assert_eq!(evidence_mechanisms.file_observations, 3);
        let paired = summary.paired_objective.unwrap();
        assert_eq!(paired.expected_pairs, 3);
        assert_eq!(paired.eligible_pairs, 3);
        assert_eq!(paired.excluded_pairs, 0);
        assert_eq!(paired.candidate_wins, 2);
        assert_eq!(paired.control_wins, 1);
        assert_eq!(paired.objective_rate_delta, Some(1.0 / 3.0));
        assert!(paired.task_evidence_threshold_met);
        assert_eq!(paired.candidate_tasks_at_least_two_of_three, ["H01"]);
        assert_eq!(
            paired.candidate_task_variants_at_least_two_of_three,
            ["H01/recovery"]
        );
    }

    #[test]
    fn invalid_trace_or_unpaired_rows_are_excluded_not_scored_as_losses() {
        let mut rows = vec![
            paired_row("H01", "recovery", 1, AutonomyArm::Control, false),
            paired_row("H01", "recovery", 1, AutonomyArm::Candidate, false),
            paired_row("H01", "recovery", 2, AutonomyArm::Control, false),
            paired_row("H01", "recovery", 2, AutonomyArm::Candidate, true),
            paired_row("H01", "recovery", 3, AutonomyArm::Candidate, false),
        ];
        rows[0].infrastructure_error =
            Some("trace structure finish rejected the trace".to_string());
        for index in [0, 1, 4] {
            rows[index].contract_passed = false;
            rows[index].clarification_expected = true;
            rows[index].clarification_observed = true;
            rows[index].clarification_correct = false;
            rows[index].unnecessary_clarification = true;
            rows[index].resumes_expected = 1;
            rows[index].resumes_observed = 0;
            rows[index].recovery_succeeded = false;
            rows[index].duplicate_effects_within_limit = Some(false);
            rows[index].command_checks.push(CommandCheckResult {
                name: "excluded model failure".to_string(),
                status: CommandCheckStatus::ModelFailure,
                exit_code: Some(1),
                timed_out: false,
                stdout_excerpt: String::new(),
                stderr_excerpt: String::new(),
                reason: Some("excluded pair".to_string()),
            });
            rows[index].metrics.observations_recorded = 10;
            rows[index].metrics.tools_called = vec!["excluded_tool".to_string()];
        }
        rows[3].metrics.observations_recorded = 1;
        rows[3].metrics.tools_called = vec!["eligible_tool".to_string()];
        let summary = summarize_autonomy_run_with_coordinates(
            "run",
            "autonomy-v1",
            1,
            &"5".repeat(64),
            1,
            2,
            3,
            &["H01".to_string()],
            &categories(&[("H01", "long_horizon")]),
            &["recovery".to_string()],
            &AutonomyEvaluationCoordinate::paired(),
            &rows,
            Vec::new(),
        );
        assert!(!summary.complete);
        assert_eq!(summary.overall.observed, 5);
        assert_eq!(summary.overall.scoreable, 2);
        assert_eq!(summary.overall.infrastructure_failures, 3);
        assert_eq!(summary.overall.contract_passes, 2);
        assert_eq!(summary.overall.objective_completions, 1);

        let legacy = summary
            .by_harness_policy
            .iter()
            .find(|rate| rate.key == HarnessPolicy::Legacy.label())
            .unwrap();
        assert_eq!((legacy.observed, legacy.scoreable), (2, 1));
        assert_eq!(legacy.objective_completions, 0);
        let evidence = summary
            .by_harness_policy
            .iter()
            .find(|rate| rate.key == HarnessPolicy::Evidence.label())
            .unwrap();
        assert_eq!((evidence.observed, evidence.scoreable), (3, 1));
        assert_eq!(evidence.objective_completions, 1);

        let control = summary
            .by_arm
            .iter()
            .find(|rate| rate.key == AutonomyArm::Control.label())
            .unwrap();
        assert_eq!((control.observed, control.scoreable), (2, 1));
        let candidate = summary
            .by_arm
            .iter()
            .find(|rate| rate.key == AutonomyArm::Candidate.label())
            .unwrap();
        assert_eq!((candidate.observed, candidate.scoreable), (3, 1));

        let evidence_mechanisms = summary
            .by_policy_mechanisms
            .iter()
            .find(|mechanisms| mechanisms.harness_policy == HarnessPolicy::Evidence)
            .unwrap();
        assert_eq!(evidence_mechanisms.scoreable_episodes, 1);
        assert_eq!(evidence_mechanisms.observations_recorded, 1);
        assert!(
            summary
                .by_tool
                .iter()
                .any(|tool| tool.tool == "eligible_tool")
        );
        assert!(
            !summary
                .by_tool
                .iter()
                .any(|tool| tool.tool == "excluded_tool")
        );
        assert_eq!(summary.clarification.required, 0);
        assert_eq!(summary.recovery.episodes_expected, 0);
        assert_eq!(summary.turns.samples, 2);
        assert_eq!(summary.failure_counts.get("infrastructure"), Some(&1));
        assert_eq!(summary.failure_counts.get("objective_incomplete"), Some(&1));
        assert!(!summary.failure_counts.contains_key("contract"));
        assert!(
            !summary
                .failure_counts
                .contains_key("unnecessary_clarification")
        );
        assert!(
            !summary
                .failure_counts
                .contains_key("clarification_missed_or_incorrect")
        );
        assert!(!summary.failure_counts.contains_key("recovery"));
        assert!(
            !summary
                .failure_counts
                .contains_key("duplicate_or_collateral_effect")
        );
        assert!(!summary.failure_counts.contains_key("check"));
        let paired = summary.paired_objective.unwrap();
        assert_eq!(paired.expected_pairs, 3);
        assert_eq!(paired.eligible_pairs, 1);
        assert_eq!(paired.excluded_pairs, 2);
        assert_eq!(paired.candidate_wins, 1);
        assert_eq!(paired.control_wins, 0);
        assert_eq!(paired.objective_rate_delta, Some(1.0));
    }

    #[test]
    fn repository_brief_comparisons_never_cross_policy_coordinates() {
        let rows = vec![
            paired_row("H01", "recovery", 1, AutonomyArm::Control, true),
            paired_row("H01", "recovery", 1, AutonomyArm::Candidate, false),
            paired_row("H01", "repository_brief", 1, AutonomyArm::Control, false),
            paired_row("H01", "repository_brief", 1, AutonomyArm::Candidate, true),
        ];
        let summary = summarize_autonomy_run_with_coordinates(
            "run",
            "autonomy-v1",
            1,
            &"5".repeat(64),
            1,
            2,
            1,
            &["H01".to_string()],
            &categories(&[("H01", "long_horizon")]),
            &["recovery".to_string(), "repository_brief".to_string()],
            &AutonomyEvaluationCoordinate::paired(),
            &rows,
            Vec::new(),
        );
        assert!(summary.complete);
        assert_eq!(summary.repository_brief_ab.paired_episodes, 2);
        let legacy = summary
            .repository_brief_ab_by_policy
            .iter()
            .find(|summary| summary.harness_policy == HarnessPolicy::Legacy)
            .unwrap();
        assert_eq!(legacy.comparison.paired_episodes, 1);
        assert_eq!(legacy.comparison.recovery_wins, 1);
        let evidence = summary
            .repository_brief_ab_by_policy
            .iter()
            .find(|summary| summary.harness_policy == HarnessPolicy::Evidence)
            .unwrap();
        assert_eq!(evidence.comparison.paired_episodes, 1);
        assert_eq!(evidence.comparison.repository_brief_wins, 1);
    }

    #[test]
    fn paired_freshness_requires_distinct_instances_and_equal_initial_trees() {
        let control = paired_row("H01", "recovery", 1, AutonomyArm::Control, false);
        let mut candidate = paired_row("H01", "recovery", 1, AutonomyArm::Candidate, true);
        candidate.evaluation_provenance.workspace_instance_sha256 = control
            .evaluation_provenance
            .workspace_instance_sha256
            .clone();
        assert!(paired_rows(&[&control, &candidate]).is_none());

        candidate.evaluation_provenance.workspace_instance_sha256 = Some("7".repeat(64));
        candidate.evaluation_provenance.initial_tree_sha256 = Some("8".repeat(64));
        assert!(paired_rows(&[&control, &candidate]).is_none());
    }

    #[test]
    fn paired_provenance_requires_valid_digests_and_canonical_pair_id() {
        let control = paired_row("H01", "recovery", 1, AutonomyArm::Control, false);
        let candidate = paired_row("H01", "recovery", 1, AutonomyArm::Candidate, true);
        assert!(paired_rows(&[&control, &candidate]).is_some());

        let mut bad_workspace = candidate.clone();
        bad_workspace
            .evaluation_provenance
            .workspace_instance_sha256 = Some("not-a-digest".to_string());
        assert!(paired_rows(&[&control, &bad_workspace]).is_none());
        assert!(!strict_persisted_row_valid(&bad_workspace));

        let mut bad_tree = candidate.clone();
        bad_tree.evaluation_provenance.initial_tree_sha256 = Some("not-a-digest".to_string());
        assert!(paired_rows(&[&control, &bad_tree]).is_none());
        assert!(!strict_persisted_row_valid(&bad_tree));

        let mut wrong_control_id = control.clone();
        let mut wrong_candidate_id = candidate;
        wrong_control_id.evaluation_provenance.pair_id = Some("pair-1".to_string());
        wrong_candidate_id.evaluation_provenance.pair_id = Some("pair-1".to_string());
        assert!(paired_rows(&[&wrong_control_id, &wrong_candidate_id]).is_none());
    }

    #[test]
    fn strict_rows_fail_closed_on_trace_or_managed_server_tampering() {
        let row = paired_row("H01", "recovery", 1, AutonomyArm::Candidate, true);
        assert!(strict_persisted_row_valid(&row));
        assert!(row_is_scoreable(&row));

        let mut cases = Vec::new();
        let mut missing_trace_validation = row.clone();
        missing_trace_validation.segments[0].trace_validation = None;
        cases.push(missing_trace_validation);

        let mut timed_out = row.clone();
        timed_out.segments[0].timed_out = true;
        cases.push(timed_out);

        let mut missing_terminal = row.clone();
        missing_terminal.segments[0].observed_terminal = None;
        cases.push(missing_terminal);

        let mut bad_trace_digest = row.clone();
        bad_trace_digest.segments[0].trace_sha256 = Some("not-a-digest".to_string());
        cases.push(bad_trace_digest);

        let mut wrong_engine = row.clone();
        wrong_engine
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .engine = "ollama".to_string();
        cases.push(wrong_engine);

        let mut wrong_context = row.clone();
        wrong_context
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .context_size = Some(4096);
        cases.push(wrong_context);

        let mut random_seed = row.clone();
        random_seed
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .sampling_seed = Some(-1);
        cases.push(random_seed);

        let mut wrong_owner = row.clone();
        wrong_owner
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .listener_owner_pid = Some(9999);
        cases.push(wrong_owner);

        let mut split_listener_url = row.clone();
        split_listener_url
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .listener_base_url = "http://127.0.0.1:8081/v1".to_string();
        cases.push(split_listener_url);

        let mut same_basename_other_path = row.clone();
        same_basename_other_path
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .model = Some(
            std::env::current_dir()
                .unwrap()
                .join("other/model.gguf")
                .display()
                .to_string(),
        );
        cases.push(same_basename_other_path);

        let mut gpu_enabled = row.clone();
        gpu_enabled
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .gpu_layers = Some(1);
        cases.push(gpu_enabled);

        let mut tampered_argv = row.clone();
        tampered_argv
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .engine_argv
            .as_mut()
            .unwrap()[2] = "other.gguf".to_string();
        cases.push(tampered_argv);

        let mut wrong_model_hash = row.clone();
        wrong_model_hash
            .evaluation_provenance
            .managed_server
            .as_mut()
            .unwrap()
            .model_sha256 = Some("f".repeat(64));
        cases.push(wrong_model_hash);

        for tampered in cases {
            assert!(!strict_persisted_row_valid(&tampered));
            assert!(!row_is_scoreable(&tampered));
        }
    }

    #[test]
    fn strict_validated_timeout_is_a_scoreable_model_failure() {
        let mut row = paired_row("H01", "recovery", 1, AutonomyArm::Candidate, false);
        row.contract_passed = false;
        row.objective_completed = false;
        row.final_terminal = None;
        row.segments[0].observed_terminal = None;
        row.segments[0].exit_code = None;
        row.segments[0].timed_out = true;

        assert!(strict_persisted_row_valid(&row));
        assert!(row_is_scoreable(&row));

        let mut with_exit_code = row.clone();
        with_exit_code.segments[0].exit_code = Some(1);
        assert!(!strict_persisted_row_valid(&with_exit_code));

        let mut missing_path = row.clone();
        missing_path.segments[0].trace_path = None;
        assert!(!strict_persisted_row_valid(&missing_path));

        let mut missing_digest = row.clone();
        missing_digest.segments[0].trace_sha256 = None;
        assert!(!strict_persisted_row_valid(&missing_digest));

        let mut unvalidated = row.clone();
        unvalidated.segments[0].trace_validation = None;
        assert!(!strict_persisted_row_valid(&unvalidated));

        let mut with_segment_terminal = row.clone();
        with_segment_terminal.segments[0].observed_terminal = Some("max_turns".to_string());
        assert!(!strict_persisted_row_valid(&with_segment_terminal));

        let mut with_final_terminal = row.clone();
        with_final_terminal.final_terminal = Some("max_turns".to_string());
        assert!(!strict_persisted_row_valid(&with_final_terminal));

        let mut with_grading = row.clone();
        with_grading.command_checks.push(CommandCheckResult {
            name: "must be skipped".to_string(),
            status: CommandCheckStatus::ModelFailure,
            exit_code: Some(1),
            timed_out: false,
            stdout_excerpt: String::new(),
            stderr_excerpt: String::new(),
            reason: Some("grader ran after timeout".to_string()),
        });
        assert!(!strict_persisted_row_valid(&with_grading));

        let mut followed_by_segment = row.clone();
        followed_by_segment.segments.push(
            paired_row("H01", "recovery", 1, AutonomyArm::Candidate, false).segments[0].clone(),
        );
        followed_by_segment.segments[1].segment = 2;
        assert!(!strict_persisted_row_valid(&followed_by_segment));

        let mut claimed_success = row;
        claimed_success.objective_completed = true;
        assert!(!strict_persisted_row_valid(&claimed_success));
    }

    #[test]
    fn single_evidence_rows_use_the_same_strict_persisted_predicate() {
        let mut row = paired_row("H01", "current", 1, AutonomyArm::Candidate, true);
        row.arm = AutonomyArm::Single;
        row.evaluation_provenance.arm = AutonomyArm::Single;
        row.evaluation_provenance.pair_id = None;
        row.evaluation_provenance.pair_slot = None;
        row.evaluation_provenance.pair_order = None;
        assert!(strict_persisted_row_valid(&row));
        row.evaluation_provenance.managed_server = None;
        assert!(!row_is_scoreable(&row));
    }

    #[test]
    fn schema_one_rows_default_to_single_legacy_coordinates() {
        let row = row("run", "A01", "ambiguity", "current", 1);
        let mut value = serde_json::to_value(row).unwrap();
        let object = value.as_object_mut().unwrap();
        object.insert("schema_version".to_string(), serde_json::json!(1));
        object.remove("arm");
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
        assert_eq!(decoded.arm, AutonomyArm::Single);
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
