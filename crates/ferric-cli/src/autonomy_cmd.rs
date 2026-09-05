//! Operational runner for Ferric's versioned internal autonomy matrix.
//!
//! Every episode uses the real `ferric query` process boundary. Recovery and
//! repository-brief variants chain the trace produced by one process into the
//! next; there is no offline/demo execution path.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use ferric_bench::{
    AUTONOMY_RESULTS_SCHEMA_VERSION, AutonomyArm, AutonomyCategory, AutonomyCheck,
    AutonomyEvaluationCoordinate, AutonomyEvaluationProvenance, AutonomyResultRow,
    AutonomyRunIssue, AutonomySegmentResult, AutonomyTask, AutonomyTraceMetrics, BinaryProvenance,
    EMBEDDED_AUTONOMY_V1, Invocation, ManagedServerProvenance, ModelProvenance, OpenAiArgs,
    QuerySegmentRequest, RecoveryInjectionKind, ResumeProbeResult, ResumeRefusalMode,
    RetainedTraceValidation, RunProvenance, TerminalOutcome, append_autonomy_row,
    autonomy_bench_spec, embedded_autonomy_suite, generate_repository_brief,
    preflight_command_checks, run_query_segment, summarize_autonomy_run_with_coordinates,
    verify_command_checks_with_deadline, write_autonomy_summary,
};
use ferric_core::{ActionProtocol, HarnessPolicy};
use ferric_loop::TraceStructure;
use ferric_trace::{
    ControllerBlockReason, Event, ObservationDetailV1, TRACE_SCHEMA_VERSION, VerificationCheckV1,
    VerificationOutcome,
};

use crate::query::ProtocolArg;

const PROVIDER_FAILURE_ENDPOINT: &str = "http://127.0.0.1:0/v1";
const STDERR_TAIL_BYTES: usize = 1_000;
const STRICT_AUTONOMY_CONTEXT: u32 = 8192;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutonomyVariantArg {
    Current,
    Recovery,
    RepositoryBrief,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum ServerStateArg {
    Unknown,
    Cold,
    Warm,
}

/// The autonomy matrix intentionally excludes the experimental planner arm:
/// this sprint establishes only legacy/evidence controller causality.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum AutonomyHarnessPolicyArg {
    Legacy,
    Evidence,
}

impl From<AutonomyHarnessPolicyArg> for HarnessPolicy {
    fn from(policy: AutonomyHarnessPolicyArg) -> Self {
        match policy {
            AutonomyHarnessPolicyArg::Legacy => HarnessPolicy::Legacy,
            AutonomyHarnessPolicyArg::Evidence => HarnessPolicy::Evidence,
        }
    }
}

impl ServerStateArg {
    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Cold => "cold",
            Self::Warm => "warm",
        }
    }
}

impl AutonomyVariantArg {
    fn label(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Recovery => "recovery",
            Self::RepositoryBrief => "repository_brief",
        }
    }
}

#[derive(Args)]
pub struct AutonomyArgs {
    /// Task IDs to run (repeatable). Empty selects all 24 frozen tasks.
    #[arg(long)]
    pub task: Vec<String>,

    /// Policy variants to run (repeatable). Empty selects all three variants.
    #[arg(long, value_enum)]
    pub variant: Vec<AutonomyVariantArg>,

    /// Number of independent task×variant repetitions.
    #[arg(
        long,
        default_value_t = 1,
        value_parser = clap::value_parser!(u32).range(1..=100)
    )]
    pub trials: u32,

    #[arg(long, value_enum, default_value = "grammar")]
    pub protocol: ProtocolArg,

    /// OpenAI-compatible endpoint. Omit to discover a running `ferric server`.
    #[arg(long)]
    pub api_base: Option<String>,

    /// Model identifier exposed by the running OpenAI-compatible server.
    #[arg(long)]
    pub model: Option<String>,

    /// SHA-256 of the model artifact when known.
    #[arg(long, value_parser = parse_sha256)]
    pub model_sha256: Option<String>,

    #[arg(long, default_value_t = 1.2)]
    pub params_b: f32,

    #[arg(long, default_value_t = 4096)]
    pub ctx: u32,

    /// Whether the model server was process-cold or already warm when this
    /// matrix invocation began. This is operator-observed provenance.
    #[arg(long, value_enum, default_value = "unknown")]
    pub server_state: ServerStateArg,

    #[arg(long, default_value = "python")]
    pub python_bin: PathBuf,

    #[arg(long, default_value = "benchmarks/autonomy")]
    pub results_dir: PathBuf,

    #[arg(long)]
    pub ferric_bin: Option<PathBuf>,

    /// Harness policy for a single-binary run. Omission preserves the exact
    /// historical legacy child argv.
    #[arg(long, value_enum)]
    pub harness_policy: Option<AutonomyHarnessPolicyArg>,

    /// Run adjacent counterbalanced frozen-control/evidence-candidate pairs.
    #[arg(long)]
    pub paired: bool,

    /// Ferric executable used by the implicit-legacy control arm.
    #[arg(long)]
    pub control_bin: Option<PathBuf>,

    /// Operator-pinned digest of the known control baseline.
    #[arg(long, value_parser = parse_sha256)]
    pub control_sha256: Option<String>,

    /// Ferric executable used by the `--harness-policy evidence` arm.
    #[arg(long)]
    pub candidate_bin: Option<PathBuf>,

    /// Preserve each materialized repository after grading.
    #[arg(long)]
    pub keep_workspace: bool,

    /// Validate and print the frozen corpus without running model episodes.
    #[arg(long)]
    pub list: bool,
}

struct EpisodeCoordinate {
    coordinate: AutonomyEvaluationCoordinate,
    invocation: Invocation,
    /// `None` is part of the control contract: the frozen legacy binary never
    /// receives a flag which did not exist when it was built.
    child_policy_flag: Option<HarnessPolicy>,
    frozen_binary: Option<FrozenBinary>,
}

struct FrozenPair {
    control: FrozenBinary,
    candidate: FrozenBinary,
}

struct FrozenBinary {
    path: PathBuf,
    provenance: BinaryProvenance,
    /// On Windows this handle is opened without write/delete sharing. On other
    /// platforms it still pins the exact artifact inode while pre/post hashes
    /// detect replacement around every child spawn.
    _guard: std::fs::File,
}

struct ManagedServerBinding {
    provenance: ManagedServerProvenance,
    strict: Option<StrictManagedServerBinding>,
}

struct StrictManagedServerBinding {
    scope: crate::server::ManagedDiscoveryScope,
    initial_fingerprint: crate::server::DiscoveryFingerprint,
    initial_snapshot: crate::server::RegisteredServerSnapshot,
    query_model: String,
    query_context: u32,
    expected_model_sha256: String,
}

struct PairContext {
    id: String,
    slot: u8,
    order: &'static str,
}

struct RetainedTraceKey<'a> {
    trial: u32,
    task_id: &'a str,
    variant: &'a str,
    coordinate: AutonomyEvaluationCoordinate,
    segment: u32,
}

struct RetainedTrace {
    relative_path: String,
    absolute_path: PathBuf,
    sha256: String,
    bytes: Vec<u8>,
}

pub fn run_autonomy(args: AutonomyArgs) -> ExitCode {
    if let Err(error) =
        crate::config::validate_effective_numbers(args.params_b, args.ctx, 0.0, None)
    {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    let suite = match embedded_autonomy_suite() {
        Ok(suite) => suite,
        Err(error) => {
            eprintln!("autonomy corpus: {error}");
            return ExitCode::FAILURE;
        }
    };
    let tasks = match select_tasks(&suite.tasks, &args.task) {
        Ok(tasks) => tasks,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let variants = select_variants(&args.variant);

    if args.list {
        println!(
            "{} (schema {}, internal baseline only)",
            suite.name, suite.schema_version
        );
        for task in &tasks {
            println!("{}\t{:?}\t{}", task.id, task.category, task.name);
        }
        println!(
            "variants: {}",
            variants
                .iter()
                .map(|variant| variant.label())
                .collect::<Vec<_>>()
                .join(", ")
        );
        return ExitCode::SUCCESS;
    }

    if args.model.as_deref().is_none_or(str::is_empty) {
        eprintln!("--model <id> is required for an autonomy run (or use --list)");
        return ExitCode::FAILURE;
    }
    if !matches!(args.protocol, ProtocolArg::Grammar) {
        eprintln!(
            "autonomy corpus v1 requires --protocol grammar; its one-turn recovery injections are not comparable under multi-call protocols"
        );
        return ExitCode::FAILURE;
    }
    if let Err(error) = validate_binary_mode(&args) {
        eprintln!("autonomy binary mode: {error}");
        return ExitCode::FAILURE;
    }
    let strict_mode = strict_managed_mode(&args);
    let endpoint = if strict_mode {
        let workspace = match std::env::current_dir() {
            Ok(workspace) => workspace,
            Err(error) => {
                eprintln!("autonomy server discovery: resolve current directory: {error}");
                return ExitCode::FAILURE;
            }
        };
        let scope = match crate::server::ManagedDiscoveryScope::for_workspace(&workspace) {
            Ok(scope) => scope,
            Err(error) => {
                eprintln!("autonomy server discovery: {error}");
                return ExitCode::FAILURE;
            }
        };
        let discovery = crate::server::discover_managed_server_in(&scope);
        crate::backend::require_managed_endpoint(scope, discovery, args.api_base.as_deref())
    } else {
        crate::backend::resolved_endpoint(args.api_base.as_deref())
    };
    let endpoint = match endpoint {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("autonomy server discovery: {error}");
            return ExitCode::FAILURE;
        }
    };
    let resolved_api_base = endpoint.base_url().to_string();
    if let Err(error) = preflight_autonomy_checks(&tasks, &args.python_bin) {
        eprintln!("autonomy check infrastructure: {error}");
        return ExitCode::FAILURE;
    }
    let managed_server = match managed_server_binding(
        &endpoint,
        args.model.as_deref().expect("validated model"),
        args.model_sha256.as_deref(),
        args.ctx,
        strict_mode,
    ) {
        Ok(provenance) => provenance,
        Err(error) => {
            eprintln!("autonomy managed-server provenance: {error}");
            return ExitCode::FAILURE;
        }
    };
    let server_preflight = managed_server
        .as_ref()
        .and_then(|binding| binding.strict.as_ref())
        .map_or_else(
            || {
                preflight_openai_server(
                    &resolved_api_base,
                    args.model.as_deref().expect("validated model"),
                )
            },
            |strict| {
                crate::server::with_registered_server_effect(
                    &strict.initial_fingerprint.runfile,
                    || {
                        preflight_openai_server(
                            &resolved_api_base,
                            args.model.as_deref().expect("validated model"),
                        )
                    },
                )
                .map(|_| ())
            },
        );
    if let Err(error) = server_preflight {
        eprintln!("autonomy server preflight: {error}");
        return ExitCode::FAILURE;
    }

    let protocol: ActionProtocol = args.protocol.into();
    let openai = Some(OpenAiArgs {
        api_base: Some(resolved_api_base),
        model: args.model.clone().expect("validated model"),
        params_b: args.params_b,
        ctx: args.ctx,
    });
    let started_at = now_unix_ms();
    let run_id = new_run_id(started_at);
    let coordinates = if args.paired {
        let frozen = match freeze_paired_binaries(
            &args.results_dir,
            &run_id,
            args.control_bin.as_deref().expect("validated control"),
            args.candidate_bin.as_deref().expect("validated candidate"),
            args.control_sha256
                .as_deref()
                .expect("validated control SHA-256"),
        ) {
            Ok(frozen) => frozen,
            Err(error) => {
                eprintln!("autonomy paired binaries: {error}");
                return ExitCode::FAILURE;
            }
        };
        vec![
            EpisodeCoordinate {
                coordinate: AutonomyEvaluationCoordinate {
                    arm: AutonomyArm::Control,
                    harness_policy: HarnessPolicy::Legacy,
                },
                invocation: Invocation {
                    ferric_bin: frozen.control.path.clone(),
                    protocol,
                    openai: openai.clone(),
                    prompts_dir: None,
                    keep_workspace: args.keep_workspace,
                },
                child_policy_flag: None,
                frozen_binary: Some(frozen.control),
            },
            EpisodeCoordinate {
                coordinate: AutonomyEvaluationCoordinate {
                    arm: AutonomyArm::Candidate,
                    harness_policy: HarnessPolicy::Evidence,
                },
                invocation: Invocation {
                    ferric_bin: frozen.candidate.path.clone(),
                    protocol,
                    openai: openai.clone(),
                    prompts_dir: None,
                    keep_workspace: args.keep_workspace,
                },
                child_policy_flag: Some(HarnessPolicy::Evidence),
                frozen_binary: Some(frozen.candidate),
            },
        ]
    } else {
        let ferric_bin = args
            .ferric_bin
            .clone()
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_else(|| PathBuf::from("ferric"));
        let (coordinate, child_policy_flag) = single_evaluation_coordinate(&args);
        let (invocation_binary, frozen_binary) = match prepare_single_binary(
            &args.results_dir,
            &run_id,
            coordinate.harness_policy,
            ferric_bin,
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("autonomy evidence binary: {error}");
                return ExitCode::FAILURE;
            }
        };
        vec![EpisodeCoordinate {
            coordinate,
            invocation: Invocation {
                ferric_bin: invocation_binary,
                protocol,
                openai,
                prompts_dir: None,
                keep_workspace: args.keep_workspace,
            },
            child_policy_flag,
            frozen_binary,
        }]
    };
    let suite_sha256 = ferric_bench::sha256_bytes(EMBEDDED_AUTONOMY_V1.as_bytes());
    let mut rows = Vec::new();
    let mut issues = Vec::new();

    for trial in 1..=args.trials {
        let offset = (trial - 1) as usize % tasks.len();
        for position in 0..tasks.len() {
            let task = tasks[(position + offset) % tasks.len()];
            let variant_offset = ((trial - 1) as usize + position) % variants.len();
            for variant_position in 0..variants.len() {
                let variant = &variants[(variant_position + variant_offset) % variants.len()];
                let coordinate_order = episode_coordinate_order(task.id.as_str(), *variant, trial);
                let order_label = if coordinate_order == [0, 1] {
                    "control_candidate"
                } else {
                    "candidate_control"
                };
                let coordinate_order: &[usize] = if args.paired { &coordinate_order } else { &[0] };
                for (slot, coordinate_index) in coordinate_order.iter().copied().enumerate() {
                    let coordinate = &coordinates[coordinate_index];
                    let pair = args.paired.then(|| PairContext {
                        id: pair_id(task.id.as_str(), *variant, trial),
                        slot: slot as u8 + 1,
                        order: order_label,
                    });
                    println!(
                        "{} {} {} {} trial-{trial:03} — running",
                        task.id,
                        variant.label(),
                        coordinate.coordinate.arm.label(),
                        coordinate.coordinate.harness_policy.label()
                    );
                    match run_episode(
                        &suite.suite_id,
                        suite.schema_version,
                        &suite_sha256,
                        task,
                        *variant,
                        trial,
                        &run_id,
                        coordinate,
                        pair.as_ref(),
                        managed_server.as_ref(),
                        &args,
                    ) {
                        Ok(mut row) => {
                            println!(
                                "{} {} {} {} trial-{trial:03} — contract {} / objective {} ({} ms)",
                                task.id,
                                variant.label(),
                                coordinate.coordinate.arm.label(),
                                coordinate.coordinate.harness_policy.label(),
                                pass_label(row.contract_passed),
                                pass_label(row.objective_completed),
                                row.wall_ms
                            );
                            if let Some(error) = &row.infrastructure_error {
                                issues.push(AutonomyRunIssue {
                                    task_id: Some(task.id.clone()),
                                    variant: Some(variant.label().to_string()),
                                    arm: Some(coordinate.coordinate.arm),
                                    harness_policy: Some(coordinate.coordinate.harness_policy),
                                    trial: Some(trial),
                                    message: error.clone(),
                                });
                            }
                            if !strict_mode {
                                append_row_or_issue(&args.results_dir, &mut row, &mut issues);
                            }
                            rows.push(row);
                        }
                        Err(error) => {
                            eprintln!(
                                "{} {} {} {} trial-{trial:03} — {error}",
                                task.id,
                                variant.label(),
                                coordinate.coordinate.arm.label(),
                                coordinate.coordinate.harness_policy.label()
                            );
                            issues.push(AutonomyRunIssue {
                                task_id: Some(task.id.clone()),
                                variant: Some(variant.label().to_string()),
                                arm: Some(coordinate.coordinate.arm),
                                harness_policy: Some(coordinate.coordinate.harness_policy),
                                trial: Some(trial),
                                message: error,
                            });
                        }
                    }
                }
            }
        }
    }

    if let Some(binding) = managed_server.as_ref()
        && let Some(strict) = binding.strict.as_ref()
        && let Err(error) = revalidate_managed_server(binding, strict)
    {
        let message = format!("post-matrix managed-server validation: {error}");
        eprintln!("{message}");
        issues.push(AutonomyRunIssue {
            task_id: None,
            variant: None,
            arm: None,
            harness_policy: None,
            trial: None,
            message: message.clone(),
        });
        for row in &mut rows {
            if row.arm != AutonomyArm::Single || row.harness_policy == HarnessPolicy::Evidence {
                row.infrastructure_error = Some(
                    row.infrastructure_error
                        .take()
                        .map_or_else(|| message.clone(), |prior| format!("{prior}; {message}")),
                );
            }
        }
    }

    // Strict rows are persisted only after the post-matrix live attestation.
    // This prevents a failed final binding from leaving apparently scoreable
    // rows in the append-only JSONL file. Historical single-legacy mode keeps
    // its existing per-episode append behavior.
    if strict_mode {
        for row in &mut rows {
            append_row_or_issue(&args.results_dir, row, &mut issues);
        }
    }

    let finished_at = now_unix_ms();
    let expected_tasks = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
    let expected_task_categories = tasks
        .iter()
        .map(|task| (task.id.clone(), category_label(task.category).to_string()))
        .collect::<BTreeMap<_, _>>();
    let expected_variants = variants
        .iter()
        .map(|variant| variant.label().to_string())
        .collect::<Vec<_>>();
    let expected_coordinates = coordinates
        .iter()
        .map(|coordinate| coordinate.coordinate)
        .collect::<Vec<_>>();
    let summary = summarize_autonomy_run_with_coordinates(
        &run_id,
        &suite.suite_id,
        suite.schema_version,
        &suite_sha256,
        started_at,
        finished_at,
        args.trials,
        &expected_tasks,
        &expected_task_categories,
        &expected_variants,
        &expected_coordinates,
        &rows,
        issues,
    );
    match write_autonomy_summary(&args.results_dir, &summary) {
        Ok(path) => println!("summary: {}", path.display()),
        Err(error) => {
            eprintln!("cannot write autonomy summary: {error}");
            return ExitCode::FAILURE;
        }
    }
    println!(
        "internal baseline only: contract {:.1}% / objective {:.1}% ({} rows)",
        summary.overall.contract_rate * 100.0,
        summary.overall.objective_rate * 100.0,
        summary.observed_rows
    );
    if let Some(paired) = &summary.paired_objective {
        let delta = paired.objective_rate_delta.map_or_else(
            || "n/a".to_string(),
            |value| format!("{:+.1} pp", value * 100.0),
        );
        println!(
            "paired objective: {delta}; eligible {}/{}; candidate 2-of-3 task evidence: {}",
            paired.eligible_pairs,
            paired.expected_pairs,
            if paired.task_evidence_threshold_met {
                "yes"
            } else {
                "no"
            }
        );
    }
    if summary.complete && summary.infrastructure_clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn append_row_or_issue(
    results_dir: &Path,
    row: &mut AutonomyResultRow,
    issues: &mut Vec<AutonomyRunIssue>,
) {
    if let Err(error) = append_autonomy_row(results_dir, row) {
        eprintln!("cannot append autonomy row: {error}");
        let message = format!("cannot append result row: {error}");
        issues.push(AutonomyRunIssue {
            task_id: Some(row.task_id.clone()),
            variant: Some(row.variant.clone()),
            arm: Some(row.arm),
            harness_policy: Some(row.harness_policy),
            trial: Some(row.trial),
            message: message.clone(),
        });
        row.infrastructure_error = Some(
            row.infrastructure_error
                .take()
                .map_or_else(|| message.clone(), |prior| format!("{prior}; {message}")),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn run_episode(
    suite_id: &str,
    suite_schema_version: u32,
    suite_sha256: &str,
    task: &AutonomyTask,
    variant: AutonomyVariantArg,
    trial: u32,
    run_id: &str,
    episode: &EpisodeCoordinate,
    pair: Option<&PairContext>,
    managed_server: Option<&ManagedServerBinding>,
    args: &AutonomyArgs,
) -> Result<AutonomyResultRow, String> {
    verify_episode_binary(episode, "before episode")?;
    let started_at = now_unix_ms();
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(task.timeout_s))
        .ok_or_else(|| format!("{} timeout is too large", task.id))?;
    let workspace = tempfile::tempdir().map_err(|error| format!("create workspace: {error}"))?;
    let canonical_workspace = std::fs::canonicalize(workspace.path())
        .map_err(|error| format!("canonicalize workspace: {error}"))?;
    let workspace_nonce = WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let workspace_instance_sha256 = ferric_bench::sha256_bytes(
        format!(
            "{run_id}\0{}\0{workspace_nonce}",
            canonical_workspace.display()
        )
        .as_bytes(),
    );
    let profile_dir =
        tempfile::tempdir().map_err(|error| format!("create profile dir: {error}"))?;
    materialize_task(workspace.path(), task)?;
    let initial_tree_sha256 = materialized_tree_sha256(workspace.path())?;
    let checks_file = profile_dir.path().join("checks.toml");
    write_named_checks(&checks_file, &task.checks, &args.python_bin)?;

    let (prompt, repository_brief_sha256, repository_brief_bytes, repository_brief_truncated) =
        if variant == AutonomyVariantArg::RepositoryBrief {
            let brief = generate_repository_brief(workspace.path(), Default::default())
                .map_err(|error| format!("generate repository brief: {error}"))?;
            let digest = ferric_bench::sha256_bytes(brief.text.as_bytes());
            let bytes = brief.text.len() as u64;
            let truncated = brief.truncated();
            (
                format!(
                    "[opt-in benchmark repository brief; metadata only]\n{}\n[task]\n{}",
                    brief.text, task.prompt
                ),
                Some(digest),
                Some(bytes),
                Some(truncated),
            )
        } else {
            (task.prompt.clone(), None, None, None)
        };
    let expected = expected_terminals(task, variant);
    let mut segments = Vec::new();
    let mut analyses: Vec<TraceAnalysis> = Vec::new();
    let mut prior_trace: Option<PathBuf> = None;
    let mut pending_answer: Option<String> = None;
    let mut infrastructure = Vec::new();
    let mut sequence_matches = true;
    let mut resumes_observed = 0_u32;
    let mut refusal_probes = Vec::new();
    let mut workspace_probe_done = false;

    for (index, expected_terminal) in expected.iter().copied().enumerate() {
        let segment = index as u32 + 1;
        let injection = injection_for(task, segment);
        let max_turns = if injection.is_some() {
            task.recovery
                .as_ref()
                .map_or(task.max_turns, |recovery| recovery.segment_turns)
        } else {
            task.max_turns
        };
        let api_base_override = match injection.map(|injection| injection.kind) {
            Some(RecoveryInjectionKind::ProviderFailure) => Some(PROVIDER_FAILURE_ENDPOINT),
            Some(RecoveryInjectionKind::ProcessCrash | RecoveryInjectionKind::GuardPause) => {
                infrastructure.push(format!(
                    "{} segment {segment} requests unsupported nondeterministic injection {:?}",
                    task.id,
                    injection.expect("present").kind
                ));
                None
            }
            _ => None,
        };
        let segment_timeout = remaining_before(deadline, task, "query segment")?;
        let supplied_answer = pending_answer.clone();
        let request = QuerySegmentRequest {
            workspace: workspace.path(),
            profile_dir: profile_dir.path(),
            checks_file: Some(&checks_file),
            prompt: prior_trace.is_none().then_some(prompt.as_str()),
            resume: prior_trace.as_deref(),
            answer: pending_answer.as_deref(),
            max_turns,
            timeout: segment_timeout,
            api_base_override,
            harness_policy: episode.child_policy_flag,
        };
        let process =
            run_verified_query_segment(episode, &request, &format!("query segment {segment}"))?;
        if let Some(error) = process.trace_discovery_error.clone() {
            infrastructure.push(error);
        }
        if process.timed_out && process.exit_code.is_some() {
            infrastructure.push(format!(
                "segment {segment} reported both timeout and an exit code"
            ));
        }
        let mut observed = None;
        let mut retained = None;
        let mut retained_sha256 = None;
        let mut trace_validation = None;
        let mut retained_resume_path = None;
        let mut analysis = None;
        if let Some(trace) = process.trace_path.as_deref() {
            match retain_autonomy_trace(
                trace,
                &args.results_dir,
                run_id,
                &RetainedTraceKey {
                    trial,
                    task_id: &task.id,
                    variant: variant.label(),
                    coordinate: episode.coordinate,
                    segment,
                },
            ) {
                Ok(snapshot) => {
                    retained = Some(snapshot.relative_path.clone());
                    retained_sha256 = Some(snapshot.sha256.clone());
                    retained_resume_path = Some(snapshot.absolute_path.clone());
                    match analyze_trace_bytes(
                        &snapshot.bytes,
                        workspace.path(),
                        episode.invocation.protocol,
                        max_turns,
                        episode.coordinate.harness_policy,
                    ) {
                        Ok(parsed) => {
                            observed = parsed.terminal.clone();
                            analysis = Some(parsed);
                            trace_validation = Some(RetainedTraceValidation::StructureValidated);
                        }
                        Err(error) => {
                            infrastructure.push(format!("segment {segment} trace: {error}"))
                        }
                    }
                }
                Err(error) => {
                    infrastructure.push(format!("retain segment {segment} trace: {error}"))
                }
            }
        } else {
            infrastructure.push(format!(
                "segment {segment} produced no retained trace{}",
                if process.timed_out {
                    " before timeout"
                } else {
                    ""
                }
            ));
        }
        let matches =
            terminal_matches(
                expected_terminal,
                observed.as_deref(),
                process.exit_code,
                process.timed_out,
            ) && injection_matches(injection.map(|value| value.kind), observed.as_deref());
        if analysis.is_some()
            && observed.is_none()
            && !process.timed_out
            && injection.map(|value| value.kind) != Some(RecoveryInjectionKind::ProcessCrash)
        {
            infrastructure.push(format!(
                "segment {segment} has a valid trace prefix but no SessionEnd"
            ));
        }
        if observed.as_deref() == Some("provider_error")
            && injection.map(|value| value.kind) != Some(RecoveryInjectionKind::ProviderFailure)
        {
            infrastructure.push(format!(
                "segment {segment} ended with an unexpected provider_error after server preflight"
            ));
        }
        sequence_matches &= matches;
        segments.push(AutonomySegmentResult {
            segment,
            expected_terminal: terminal_label(expected_terminal).to_string(),
            observed_terminal: observed.clone(),
            exit_code: process.exit_code,
            timed_out: process.timed_out,
            wall_ms: process.wall.as_millis() as u64,
            trace_path: retained,
            trace_sha256: retained_sha256,
            trace_validation,
            stderr_tail: trim_tail(&process.stderr_tail),
        });
        if let Some(parsed) = analysis {
            if let Some(previous) = analyses.last() {
                if parsed.resumed_from.as_deref() == Some(previous.session.as_str()) {
                    resumes_observed = resumes_observed.saturating_add(1);
                } else {
                    sequence_matches = false;
                    infrastructure.push(format!(
                        "segment {segment} does not link to the prior trace session"
                    ));
                }
                if let Some(answer) = supplied_answer.as_deref()
                    && !parsed
                        .resume_prompts
                        .iter()
                        .any(|prompt| prompt.contains(answer.trim()))
                {
                    sequence_matches = false;
                    infrastructure.push(format!(
                        "segment {segment} does not durably record the supplied clarification answer"
                    ));
                }
            } else if parsed.resumed_from.is_some() {
                sequence_matches = false;
                infrastructure
                    .push("initial segment unexpectedly claims a resumed session".to_string());
            }
            analyses.push(parsed);
        }
        let Some(trace) = retained_resume_path else {
            break;
        };

        if variant != AutonomyVariantArg::Current
            && matches
            && matches!(
                expected_terminal,
                TerminalOutcome::Paused | TerminalOutcome::NeedsInput
            )
            && !workspace_probe_done
            && task.recovery.as_ref().is_some_and(|recovery| {
                recovery
                    .refusal_modes
                    .contains(&ResumeRefusalMode::WorkspaceMismatch)
            })
        {
            refusal_probes.push(run_workspace_mismatch_probe(
                episode,
                &trace,
                profile_dir.path(),
                &checks_file,
                task,
                remaining_before(deadline, task, "workspace mismatch probe")?,
            )?);
            workspace_probe_done = true;
        }

        if index + 1 < expected.len() {
            let answer = if observed.as_deref() == Some("needs_input") {
                task.clarification
                    .as_ref()
                    .and_then(|contract| contract.answer.clone())
                    .or_else(|| {
                        task.recovery
                            .as_ref()
                            .and_then(|contract| contract.answer.clone())
                    })
            } else {
                None
            };
            if observed.as_deref() == Some("needs_input")
                && answer
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
            {
                sequence_matches = false;
                infrastructure.push(format!(
                    "segment {segment} needs input but the frozen corpus has no non-empty answer"
                ));
                break;
            }
            prior_trace = Some(trace);
            pending_answer = answer;
        } else {
            prior_trace = Some(trace);
        }
        if !matches {
            break;
        }
    }

    if variant != AutonomyVariantArg::Current
        && task.recovery.as_ref().is_some_and(|recovery| {
            recovery
                .refusal_modes
                .contains(&ResumeRefusalMode::CompletedSession)
        })
        && sequence_matches
        && segments
            .last()
            .and_then(|segment| segment.observed_terminal.as_deref())
            == Some("task_complete")
        && segments
            .last()
            .is_some_and(|segment| segment.exit_code == Some(0) && !segment.timed_out)
        && analyses
            .last()
            .is_some_and(|analysis| analysis.metrics.completion_gates_passed > 0)
        && let Some(trace) = prior_trace.as_deref()
    {
        refusal_probes.push(run_completed_probe(
            episode,
            trace,
            workspace.path(),
            profile_dir.path(),
            &checks_file,
            task,
            remaining_before(deadline, task, "completed-session probe")?,
        )?);
    }

    let expected_final = expected.last().copied();
    let final_terminal = segments
        .last()
        .and_then(|segment| segment.observed_terminal.clone());
    let episode_timed_out = segments.iter().any(|segment| segment.timed_out);
    let requires_completed_state = expected_final == Some(TerminalOutcome::Completed);
    let check_spec = autonomy_bench_spec(task, suite_schema_version);
    let command_checks = if requires_completed_state && !episode_timed_out {
        match remaining_before(deadline, task, "final grading") {
            Ok(remaining) => verify_command_checks_with_deadline(
                workspace.path(),
                &check_spec,
                &args.python_bin,
                remaining,
            ),
            Err(error) => {
                infrastructure.push(error);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    if !episode_timed_out && Instant::now() > deadline {
        infrastructure.push(format!(
            "{} exceeded its {}-second total episode deadline during final grading",
            task.id, task.timeout_s
        ));
    }
    if command_checks
        .iter()
        .any(|check| check.infrastructure_error())
    {
        infrastructure.push("authoritative command-check infrastructure failure".to_string());
    }
    let metrics = aggregate_metrics(&analyses);
    let checks_pass = requires_completed_state
        && !command_checks.is_empty()
        && command_checks.iter().all(|check| check.passed());
    let final_process_clean = segments
        .last()
        .is_some_and(|segment| segment.exit_code == Some(0) && !segment.timed_out);
    let final_gate_passed = analyses
        .last()
        .is_some_and(|analysis| analysis.metrics.completion_gates_passed > 0);
    let objective_completed = !episode_timed_out
        && final_process_clean
        && final_terminal.as_deref() == Some("task_complete")
        && checks_pass
        && final_gate_passed;

    let clarification_expected = task
        .clarification
        .as_ref()
        .is_some_and(|contract| contract.required)
        || task.recovery.as_ref().is_some_and(|recovery| {
            recovery
                .injections
                .iter()
                .any(|injection| injection.kind == RecoveryInjectionKind::ClarificationPause)
        });
    let questions: Vec<&str> = analyses
        .iter()
        .flat_map(|analysis| analysis.questions.iter().map(String::as_str))
        .collect();
    let clarification_observed = !questions.is_empty();
    let mutation_before_clarification = analyses
        .iter()
        .any(|analysis| analysis.mutation_before_question);
    let expected_question_terms = task
        .clarification
        .as_ref()
        .map(|contract| contract.expected_question_terms.as_slice())
        .or_else(|| {
            task.recovery
                .as_ref()
                .map(|recovery| recovery.expected_question_terms.as_slice())
        })
        .unwrap_or_default();
    let clarification_correct = if clarification_expected {
        clarification_observed
            && !mutation_before_clarification
            && question_matches_terms(&questions, expected_question_terms)
    } else {
        false
    };
    let unnecessary_clarification = clarification_observed && !clarification_expected;
    let expected_resumes = if variant == AutonomyVariantArg::Current {
        0
    } else {
        expected.len().saturating_sub(1) as u32
    };
    let probes_passed = refusal_probes
        .iter()
        .all(|probe| !probe.attempted || probe.rejected);
    let recovery_succeeded = resumes_observed == expected_resumes
        && sequence_matches
        && probes_passed
        && (!requires_completed_state || objective_completed);
    let clarification_contract =
        (!clarification_expected || clarification_correct) && !unnecessary_clarification;
    let contract_passed = !episode_timed_out
        && sequence_matches
        && probes_passed
        && clarification_contract
        && (!requires_completed_state || objective_completed)
        && infrastructure.is_empty();

    let finished_at = now_unix_ms();
    verify_episode_binary(episode, "after episode")?;
    let mut row_provenance = provenance(
        &episode.invocation,
        args,
        variant.label(),
        episode
            .frozen_binary
            .as_ref()
            .map(|binary| &binary.provenance),
    );
    if let Some(binding) = managed_server
        && binding.strict.is_some()
    {
        row_provenance.model.model = binding.provenance.model.clone();
        row_provenance.model.api_base = Some(binding.provenance.listener_base_url.clone());
        row_provenance.model.sha256 = binding.provenance.model_sha256.clone();
    }
    let child_binary_sha256 = row_provenance.binary.sha256.clone();
    let row = AutonomyResultRow {
        schema_version: AUTONOMY_RESULTS_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        suite_id: suite_id.to_string(),
        suite_schema_version,
        suite_sha256: suite_sha256.to_string(),
        task_id: task.id.clone(),
        task_name: task.name.clone(),
        category: category_label(task.category).to_string(),
        variant: variant.label().to_string(),
        arm: episode.coordinate.arm,
        harness_policy: episode.coordinate.harness_policy,
        trial,
        started_at_unix_ms: started_at,
        finished_at_unix_ms: finished_at,
        contract_passed,
        objective_completed,
        infrastructure_error: (!infrastructure.is_empty()).then(|| infrastructure.join("; ")),
        final_terminal,
        segments,
        clarification_expected,
        clarification_observed,
        clarification_correct,
        unnecessary_clarification,
        resumes_expected: expected_resumes,
        resumes_observed,
        recovery_succeeded,
        refusal_probes,
        duplicate_effects_within_limit: None,
        command_checks,
        metrics,
        wall_ms: finished_at.saturating_sub(started_at),
        repository_brief_sha256,
        repository_brief_bytes,
        repository_brief_truncated,
        server_state: args.server_state.label().to_string(),
        provenance: row_provenance,
        evaluation_provenance: AutonomyEvaluationProvenance {
            arm: episode.coordinate.arm,
            harness_policy: episode.coordinate.harness_policy,
            pair_id: pair.map(|pair| pair.id.clone()),
            pair_slot: pair.map(|pair| pair.slot),
            pair_order: pair.map(|pair| pair.order.to_string()),
            workspace_instance_sha256: Some(workspace_instance_sha256),
            initial_tree_sha256: Some(initial_tree_sha256),
            child_binary_sha256,
            corpus_sha256: Some(suite_sha256.to_string()),
            model_sha256: args.model_sha256.clone(),
            query_temperature: Some("0.0".to_string()),
            managed_server: managed_server.map(|binding| binding.provenance.clone()),
        },
    };

    if args.keep_workspace {
        let kept = workspace.keep();
        println!(
            "{} {} {} {} trial-{trial:03} workspace: {}",
            task.id,
            variant.label(),
            episode.coordinate.arm.label(),
            episode.coordinate.harness_policy.label(),
            kept.display()
        );
    }
    Ok(row)
}

fn managed_server_binding(
    endpoint: &crate::backend::EndpointSelection,
    query_model: &str,
    expected_model_sha256: Option<&str>,
    query_context: u32,
    strict: bool,
) -> Result<Option<ManagedServerBinding>, String> {
    let Some((scope, server)) = endpoint.managed() else {
        return if strict {
            Err("paired/evidence mode requires one ready managed `ferric server`".to_string())
        } else {
            Ok(None)
        };
    };
    let runfile = &server.runfile;
    let mut provenance = ManagedServerProvenance {
        engine: runfile.engine.program().to_string(),
        listener_base_url: runfile.base_url.clone(),
        model: runfile.model.clone(),
        model_launch_argument: None,
        model_sha256: None,
        context_size: runfile.context_size,
        sampling_seed: runfile.sampling_seed,
        parallel_slots: runfile.parallel_slots,
        gpu_layers: None,
        pid: None,
        listener_owner_pid: None,
        listener_port: None,
        engine_executable: None,
        engine_executable_sha256: None,
        engine_version: None,
        engine_argv: None,
    };
    let strict_binding = if strict {
        validate_strict_server_provenance(&provenance, query_model, query_context)?;
        let expected_model_sha256 = expected_model_sha256
            .ok_or_else(|| "paired/evidence mode requires model SHA-256 provenance".to_string())?;
        let managed_model = provenance
            .model
            .as_deref()
            .expect("strict server validation requires model");
        let model_launch_argument = managed_model.to_string();
        let canonical_model = strict_canonical_model_identity(managed_model, query_model)?;
        provenance.model_sha256 = Some(verify_managed_model_sha256(
            &canonical_model,
            expected_model_sha256,
        )?);
        provenance.model = Some(canonical_model);
        provenance.model_launch_argument = Some(model_launch_argument);
        let discovery_snapshot = server.ready_snapshot()?;
        let snapshot = crate::server::inspect_registered_server(runfile)?;
        if snapshot != discovery_snapshot {
            return Err(
                "managed process identity changed after ready discovery and before provenance collection"
                    .to_string(),
            );
        }
        validate_live_server_snapshot(runfile, &snapshot, query_model, query_context)?;
        provenance.pid = Some(snapshot.pid);
        provenance.listener_owner_pid = Some(snapshot.listener_owner_pid);
        provenance.listener_port = Some(runfile.port);
        provenance.engine_executable = Some(snapshot.executable.display().to_string());
        provenance.engine_executable_sha256 = Some(
            ferric_bench::sha256_file(&snapshot.executable).map_err(|error| {
                format!(
                    "hash live managed engine {}: {error}",
                    snapshot.executable.display()
                )
            })?,
        );
        provenance.engine_version = Some(engine_version(&snapshot.executable)?);
        provenance.engine_argv = Some(snapshot.argv.clone());
        provenance.gpu_layers = Some(0);
        Some(StrictManagedServerBinding {
            scope: scope.clone(),
            initial_fingerprint: server.fingerprint.clone(),
            initial_snapshot: snapshot,
            query_model: query_model.to_string(),
            query_context,
            expected_model_sha256: expected_model_sha256.to_string(),
        })
    } else {
        None
    };
    Ok(Some(ManagedServerBinding {
        provenance,
        strict: strict_binding,
    }))
}

fn validate_strict_server_provenance(
    provenance: &ManagedServerProvenance,
    query_model: &str,
    query_context: u32,
) -> Result<(), String> {
    if query_context != STRICT_AUTONOMY_CONTEXT {
        return Err(format!(
            "paired/evidence mode requires --ctx {STRICT_AUTONOMY_CONTEXT}, got {query_context}"
        ));
    }
    if provenance.engine != "llama-server" {
        return Err(format!(
            "paired/evidence mode requires managed llama-server provenance, got {}",
            provenance.engine
        ));
    }
    let managed_model = provenance
        .model
        .as_deref()
        .ok_or_else(|| "managed runfile does not record the model".to_string())?;
    strict_canonical_model_identity(managed_model, query_model)?;
    let context = provenance
        .context_size
        .ok_or_else(|| "managed runfile does not record context_size".to_string())?;
    if context != query_context {
        return Err(format!(
            "managed context_size {context} does not match query --ctx {query_context}"
        ));
    }
    let seed = provenance
        .sampling_seed
        .ok_or_else(|| "managed runfile does not record sampling_seed".to_string())?;
    if seed < 0 {
        return Err(format!(
            "paired/evidence mode requires a deterministic non-negative sampling seed, got {seed}"
        ));
    }
    let parallel = provenance
        .parallel_slots
        .ok_or_else(|| "managed runfile does not record parallel_slots".to_string())?;
    if parallel != 1 {
        return Err(format!(
            "paired/evidence mode requires exactly one managed parallel slot, got {parallel}"
        ));
    }
    Ok(())
}

fn validate_live_server_snapshot(
    runfile: &crate::server::ServerRunfile,
    snapshot: &crate::server::RegisteredServerSnapshot,
    query_model: &str,
    query_context: u32,
) -> Result<(), String> {
    if query_context != STRICT_AUTONOMY_CONTEXT {
        return Err(format!(
            "strict managed process context must be {STRICT_AUTONOMY_CONTEXT}, got {query_context}"
        ));
    }
    if snapshot.pid != runfile.pid || snapshot.listener_owner_pid != runfile.pid {
        return Err("managed process/listener PID does not match the runfile".to_string());
    }
    let exact_endpoint = format!("http://127.0.0.1:{}/v1", runfile.port);
    if runfile.base_url != exact_endpoint {
        return Err(format!(
            "strict managed listener endpoint must be {exact_endpoint}, got {}",
            runfile.base_url
        ));
    }
    if !executable_is_llama_server(&snapshot.executable) {
        return Err(format!(
            "managed PID {} executable is not llama-server: {}",
            snapshot.pid,
            snapshot.executable.display()
        ));
    }
    if snapshot
        .argv
        .first()
        .is_none_or(|program| !executable_is_llama_server(Path::new(program)))
    {
        return Err("managed process argv[0] is not llama-server".to_string());
    }
    let managed_model = runfile
        .model
        .as_deref()
        .ok_or_else(|| "managed runfile does not record the model".to_string())?;
    strict_canonical_model_identity(managed_model, query_model)?;
    require_unique_argv_value(&snapshot.argv, &["-m", "--model"], managed_model, "model")?;
    require_unique_argv_value(
        &snapshot.argv,
        &["-c", "--ctx-size"],
        &query_context.to_string(),
        "context",
    )?;
    let seed = runfile
        .sampling_seed
        .ok_or_else(|| "managed runfile does not record sampling_seed".to_string())?;
    require_unique_argv_value(
        &snapshot.argv,
        &["--seed"],
        &seed.to_string(),
        "sampling seed",
    )?;
    require_unique_argv_value(&snapshot.argv, &["--parallel"], "1", "parallel slots")?;
    require_unique_argv_value(&snapshot.argv, &["-ngl", "--gpu-layers"], "0", "GPU layers")?;
    require_unique_argv_value(&snapshot.argv, &["--host"], "127.0.0.1", "listener host")?;
    require_unique_argv_value(
        &snapshot.argv,
        &["--port"],
        &runfile.port.to_string(),
        "listener port",
    )?;
    Ok(())
}

fn require_unique_argv_value(
    argv: &[String],
    flags: &[&str],
    expected: &str,
    label: &str,
) -> Result<(), String> {
    let values = argv
        .windows(2)
        .filter(|window| flags.contains(&window[0].as_str()))
        .map(|window| window[1].as_str())
        .collect::<Vec<_>>();
    if values != [expected] {
        return Err(format!(
            "managed process {label} argv must occur exactly once as {expected:?}, got {values:?}"
        ));
    }
    Ok(())
}

fn executable_is_llama_server(path: &Path) -> bool {
    path.file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("llama-server"))
}

fn engine_version(executable: &Path) -> Result<String, String> {
    let mut child = std::process::Command::new(executable)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("inspect managed engine version: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return Err("managed engine --version exceeded 5 seconds".to_string());
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("inspect managed engine version process: {error}"));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("collect managed engine version: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "managed engine --version failed with {}",
            output.status
        ));
    }
    let mut version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        version = String::from_utf8_lossy(&output.stderr).trim().to_string();
    }
    if version.is_empty() || version.len() > 4096 {
        return Err("managed engine version output is empty or oversized".to_string());
    }
    Ok(version)
}

fn revalidate_managed_server(
    binding: &ManagedServerBinding,
    strict: &StrictManagedServerBinding,
) -> Result<(), String> {
    let pending = crate::server::begin_managed_server_discovery_in(&strict.scope);
    require_matching_pre_health_discovery(pending.discovery(), &strict.initial_fingerprint)?;
    let discovery = pending.finish();
    let fresh = require_fresh_managed_discovery(&discovery, &strict.initial_fingerprint)?;
    let discovery_snapshot = fresh.ready_snapshot()?;
    let (snapshot, ()) = crate::server::with_registered_server_effect(&fresh.runfile, || {
        preflight_openai_server(&fresh.runfile.base_url, &strict.query_model)
    })?;
    if snapshot != discovery_snapshot {
        return Err("managed process identity changed after final ready discovery".to_string());
    }
    validate_live_server_snapshot(
        &fresh.runfile,
        &snapshot,
        &strict.query_model,
        strict.query_context,
    )?;
    if snapshot != strict.initial_snapshot {
        return Err(
            "managed process executable/argv/listener identity changed during the matrix"
                .to_string(),
        );
    }
    let executable_sha256 = ferric_bench::sha256_file(&snapshot.executable)
        .map_err(|error| format!("rehash live managed engine: {error}"))?;
    if binding.provenance.engine_executable_sha256.as_deref() != Some(executable_sha256.as_str()) {
        return Err("managed engine executable SHA-256 changed during the matrix".to_string());
    }
    if binding.provenance.engine_version.as_deref()
        != Some(engine_version(&snapshot.executable)?.as_str())
    {
        return Err("managed engine version changed during the matrix".to_string());
    }
    let managed_model = fresh
        .runfile
        .model
        .as_deref()
        .ok_or_else(|| "managed runfile does not record the model".to_string())?;
    verify_managed_model_sha256(managed_model, &strict.expected_model_sha256)?;
    Ok(())
}

pub(crate) fn require_matching_pre_health_discovery<'a>(
    discovery: &'a crate::server::ManagedServerDiscovery,
    expected: &crate::server::DiscoveryFingerprint,
) -> Result<&'a crate::server::ManagedServer, String> {
    let server = match &discovery.state {
        crate::server::ManagedServerState::Degraded { server, .. }
            if server.listener == crate::server_process::ListenerState::OwnedByTarget
                && server.health == crate::server_resolution::HealthState::NotProbed =>
        {
            server
        }
        crate::server::ManagedServerState::Ready(server) => server,
        crate::server::ManagedServerState::Empty => {
            return Err("managed registration disappeared before final HTTP health".to_string());
        }
        crate::server::ManagedServerState::Degraded { issues, .. }
        | crate::server::ManagedServerState::Conflict { issues }
        | crate::server::ManagedServerState::Unverifiable { issues } => {
            return Err(format!(
                "fresh managed discovery is blocked before final HTTP health: {}",
                issues
                    .iter()
                    .map(|issue| issue.detail.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        crate::server::ManagedServerState::StaleOnly { .. } => {
            return Err("managed registration became stale before final HTTP health".to_string());
        }
    };
    if &server.fingerprint != expected {
        return Err(
            "managed registration/process fingerprint changed before final HTTP health".to_string(),
        );
    }
    Ok(server)
}

pub(crate) fn require_fresh_managed_discovery<'a>(
    discovery: &'a crate::server::ManagedServerDiscovery,
    expected: &crate::server::DiscoveryFingerprint,
) -> Result<&'a crate::server::ManagedServer, String> {
    let fresh = match &discovery.state {
        crate::server::ManagedServerState::Ready(server) => server,
        crate::server::ManagedServerState::Empty => {
            return Err("managed registration disappeared during the matrix".to_string());
        }
        crate::server::ManagedServerState::Degraded { issues, .. }
        | crate::server::ManagedServerState::Conflict { issues }
        | crate::server::ManagedServerState::Unverifiable { issues } => {
            return Err(format!(
                "fresh managed discovery is blocked: {}",
                issues
                    .iter()
                    .map(|issue| issue.detail.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        crate::server::ManagedServerState::StaleOnly { .. } => {
            return Err("managed registration became stale during the matrix".to_string());
        }
    };
    if &fresh.fingerprint != expected {
        return Err(
            "managed registration/process fingerprint changed during the matrix".to_string(),
        );
    }
    Ok(fresh)
}

#[cfg(test)]
fn managed_model_matches(managed: &str, query: &str) -> bool {
    match (
        std::fs::canonicalize(managed).ok(),
        std::fs::canonicalize(query).ok(),
    ) {
        (Some(managed), Some(query)) => managed == query,
        _ => false,
    }
}

fn strict_canonical_model_identity(managed: &str, query: &str) -> Result<String, String> {
    let managed = std::fs::canonicalize(managed).map_err(|error| {
        format!("strict mode cannot canonicalize managed model {managed:?}: {error}")
    })?;
    let query = std::fs::canonicalize(query).map_err(|error| {
        format!("strict mode cannot canonicalize query model {query:?}: {error}")
    })?;
    if managed != query {
        return Err(format!(
            "strict query model {} is not the managed artifact {}",
            query.display(),
            managed.display()
        ));
    }
    Ok(managed.display().to_string())
}

fn verify_managed_model_sha256(managed_model: &str, expected: &str) -> Result<String, String> {
    let path = std::fs::canonicalize(managed_model).map_err(|error| {
        format!("paired mode cannot resolve managed model artifact {managed_model:?}: {error}")
    })?;
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("inspect managed model artifact {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "managed model artifact is not a regular file: {}",
            path.display()
        ));
    }
    let actual = ferric_bench::sha256_file(&path)
        .map_err(|error| format!("hash managed model artifact {}: {error}", path.display()))?;
    if actual != expected {
        return Err(format!(
            "managed model SHA-256 mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(actual)
}

fn validate_binary_mode(args: &AutonomyArgs) -> Result<(), String> {
    let binary_contract = match (
        args.paired,
        args.control_bin.as_ref(),
        args.candidate_bin.as_ref(),
        args.ferric_bin.as_ref(),
    ) {
        (true, Some(_), Some(_), None) | (false, None, None, _) => Ok(()),
        (true, _, _, Some(_)) => {
            Err("--paired conflicts with the single-binary --ferric-bin option".to_string())
        }
        (true, _, _, None) => {
            Err("--paired requires both --control-bin and --candidate-bin".to_string())
        }
        (false, Some(_), _, _) | (false, _, Some(_), _) => {
            Err("--control-bin and --candidate-bin require --paired".to_string())
        }
    };
    binary_contract?;
    if args.paired && args.harness_policy.is_some() {
        return Err(
            "--harness-policy is a single-binary option and conflicts with --paired".to_string(),
        );
    }
    if args.paired && args.control_sha256.is_none() {
        return Err("--paired requires --control-sha256 for the frozen baseline".to_string());
    }
    if !args.paired && args.control_sha256.is_some() {
        return Err("--control-sha256 requires --paired".to_string());
    }
    if strict_managed_mode(args) && args.model_sha256.is_none() {
        return Err(
            "paired/evidence mode requires --model-sha256 for immutable model provenance"
                .to_string(),
        );
    }
    if strict_managed_mode(args) && args.ctx != STRICT_AUTONOMY_CONTEXT {
        return Err(format!(
            "paired/evidence mode requires --ctx {STRICT_AUTONOMY_CONTEXT}"
        ));
    }
    Ok(())
}

fn strict_managed_mode(args: &AutonomyArgs) -> bool {
    args.paired || args.harness_policy == Some(AutonomyHarnessPolicyArg::Evidence)
}

fn single_evaluation_coordinate(
    args: &AutonomyArgs,
) -> (AutonomyEvaluationCoordinate, Option<HarnessPolicy>) {
    let child_policy = args.harness_policy.map(HarnessPolicy::from);
    (
        AutonomyEvaluationCoordinate {
            arm: AutonomyArm::Single,
            harness_policy: child_policy.unwrap_or(HarnessPolicy::Legacy),
        },
        child_policy,
    )
}

fn freeze_paired_binaries(
    results_dir: &Path,
    run_id: &str,
    control: &Path,
    candidate: &Path,
    expected_control_sha256: &str,
) -> Result<FrozenPair, String> {
    let control = checked_binary(control, "control")?;
    let candidate = checked_binary(candidate, "candidate")?;
    let control_sha = ferric_bench::sha256_file(&control)
        .map_err(|error| format!("hash control binary: {error}"))?;
    let candidate_sha = ferric_bench::sha256_file(&candidate)
        .map_err(|error| format!("hash candidate binary: {error}"))?;
    if control_sha != expected_control_sha256 {
        return Err(format!(
            "control binary SHA-256 mismatch: expected {expected_control_sha256}, got {control_sha}"
        ));
    }
    if control_sha == candidate_sha {
        return Err(format!(
            "control and candidate binaries have the same SHA-256 {control_sha}"
        ));
    }

    let artifact_dir = results_dir.join("artifacts").join(run_id);
    std::fs::create_dir_all(&artifact_dir)
        .map_err(|error| format!("create frozen artifact directory: {error}"))?;
    let control_artifact = artifact_dir.join(binary_artifact_name("control", &control));
    let candidate_artifact = artifact_dir.join(binary_artifact_name("candidate", &candidate));
    copy_immutable_binary(&control, &control_artifact, &control_sha, "control")?;
    copy_immutable_binary(&candidate, &candidate_artifact, &candidate_sha, "candidate")?;
    seal_artifact_directory(&artifact_dir)?;
    Ok(FrozenPair {
        control: open_frozen_binary(control_artifact, &control_sha, "control")?,
        candidate: open_frozen_binary(candidate_artifact, &candidate_sha, "candidate")?,
    })
}

fn freeze_single_evidence_binary(
    results_dir: &Path,
    run_id: &str,
    source: &Path,
) -> Result<FrozenBinary, String> {
    let source = checked_binary(source, "single evidence")?;
    let sha256 = ferric_bench::sha256_file(&source)
        .map_err(|error| format!("hash single evidence binary: {error}"))?;
    let artifact_dir = results_dir.join("artifacts").join(run_id);
    std::fs::create_dir_all(&artifact_dir)
        .map_err(|error| format!("create frozen artifact directory: {error}"))?;
    let artifact = artifact_dir.join(binary_artifact_name("single-evidence", &source));
    copy_immutable_binary(&source, &artifact, &sha256, "single evidence")?;
    seal_artifact_directory(&artifact_dir)?;
    open_frozen_binary(artifact, &sha256, "single evidence")
}

fn prepare_single_binary(
    results_dir: &Path,
    run_id: &str,
    harness_policy: HarnessPolicy,
    source: PathBuf,
) -> Result<(PathBuf, Option<FrozenBinary>), String> {
    if harness_policy == HarnessPolicy::Legacy {
        return Ok((source, None));
    }
    let frozen = freeze_single_evidence_binary(results_dir, run_id, &source)?;
    Ok((frozen.path.clone(), Some(frozen)))
}

fn open_frozen_binary(path: PathBuf, sha256: &str, arm: &str) -> Result<FrozenBinary, String> {
    #[cfg(windows)]
    let guard = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
    };
    #[cfg(not(windows))]
    let guard = std::fs::File::open(&path);
    let guard = guard
        .map_err(|error| format!("open frozen {arm} binary guard {}: {error}", path.display()))?;
    let provenance = frozen_binary_provenance(&path, sha256)?;
    Ok(FrozenBinary {
        path,
        provenance,
        _guard: guard,
    })
}

fn seal_artifact_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("make frozen artifact directory private: {error}"))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn verify_episode_binary(episode: &EpisodeCoordinate, phase: &str) -> Result<(), String> {
    let Some(binary) = episode.frozen_binary.as_ref() else {
        return Ok(());
    };
    let expected = binary
        .provenance
        .sha256
        .as_deref()
        .ok_or_else(|| format!("{phase}: frozen binary has no expected SHA-256"))?;
    let metadata = std::fs::metadata(&binary.path)
        .map_err(|error| format!("{phase}: inspect frozen binary: {error}"))?;
    if !metadata.is_file() || Some(metadata.len()) != binary.provenance.size_bytes {
        return Err(format!(
            "{phase}: frozen {} binary identity/size changed",
            episode.coordinate.arm.label()
        ));
    }
    let actual = ferric_bench::sha256_file(&binary.path)
        .map_err(|error| format!("{phase}: hash frozen binary: {error}"))?;
    if actual != expected {
        return Err(format!(
            "{phase}: frozen {} binary SHA-256 changed: expected {expected}, got {actual}",
            episode.coordinate.arm.label()
        ));
    }
    Ok(())
}

fn run_verified_query_segment(
    episode: &EpisodeCoordinate,
    request: &QuerySegmentRequest<'_>,
    label: &str,
) -> Result<ferric_bench::QuerySegmentRecord, String> {
    verify_episode_binary(episode, &format!("before {label}"))?;
    let process = run_query_segment(&episode.invocation, request)
        .map_err(|error| format!("spawn {label}: {error}"));
    let after = verify_episode_binary(episode, &format!("after {label}"));
    match (process, after) {
        (Ok(process), Ok(())) => Ok(process),
        (Err(process), Ok(())) => Err(process),
        (Ok(_), Err(identity)) => Err(identity),
        (Err(process), Err(identity)) => Err(format!("{process}; {identity}")),
    }
}

fn frozen_binary_provenance(path: &Path, sha256: &str) -> Result<BinaryProvenance, String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("canonicalize frozen binary {}: {error}", path.display()))?;
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| format!("inspect frozen binary {}: {error}", canonical.display()))?;
    Ok(BinaryProvenance {
        path: canonical.display().to_string(),
        size_bytes: Some(metadata.len()),
        modified_at_unix_ms: metadata.modified().ok().and_then(system_time_unix_ms),
        sha256: Some(sha256.to_string()),
    })
}

fn checked_binary(path: &Path, arm: &str) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("resolve {arm} binary {}: {error}", path.display()))?;
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| format!("inspect {arm} binary {}: {error}", canonical.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "{arm} binary is not a regular file: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn binary_artifact_name(arm: &str, source: &Path) -> String {
    match source.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if !extension.is_empty() => format!("{arm}-ferric.{extension}"),
        _ => format!("{arm}-ferric"),
    }
}

fn copy_immutable_binary(
    source: &Path,
    destination: &Path,
    expected_sha256: &str,
    arm: &str,
) -> Result<(), String> {
    copy_immutable_binary_with(source, destination, expected_sha256, arm, || {})
}

fn copy_immutable_binary_with(
    source: &Path,
    destination: &Path,
    expected_sha256: &str,
    arm: &str,
    after_copy: impl FnOnce(),
) -> Result<(), String> {
    let mut input = std::fs::File::open(source)
        .map_err(|error| format!("open {arm} binary for freezing: {error}"))?;
    let mut output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| format!("create frozen {arm} binary: {error}"))?;
    std::io::copy(&mut input, &mut output)
        .map_err(|error| format!("copy frozen {arm} binary: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("sync frozen {arm} binary: {error}"))?;
    drop(output);

    let actual_sha256 = ferric_bench::sha256_file(destination)
        .map_err(|error| format!("hash frozen {arm} binary: {error}"))?;
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "frozen {arm} binary digest mismatch: expected {expected_sha256}, got {actual_sha256}"
        ));
    }
    after_copy();
    let source_sha256 = ferric_bench::sha256_file(source)
        .map_err(|error| format!("rehash source {arm} binary: {error}"))?;
    if source_sha256 != expected_sha256 {
        return Err(format!(
            "source {arm} binary changed while it was frozen: expected {expected_sha256}, got {source_sha256}"
        ));
    }
    let mut permissions = std::fs::metadata(source)
        .map_err(|error| format!("inspect source {arm} binary permissions: {error}"))?
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(destination, permissions)
        .map_err(|error| format!("make frozen {arm} binary read-only: {error}"))?;
    Ok(())
}

fn episode_coordinate_order(task_id: &str, variant: AutonomyVariantArg, trial: u32) -> [usize; 2] {
    let stable_parity = task_id
        .bytes()
        .chain(variant.label().bytes())
        .fold(u64::from(trial), |sum, byte| sum + u64::from(byte))
        % 2;
    if stable_parity == 0 { [0, 1] } else { [1, 0] }
}

fn pair_id(task_id: &str, variant: AutonomyVariantArg, trial: u32) -> String {
    format!("trial-{trial:03}-{task_id}-{}", variant.label())
}

fn select_tasks<'a>(
    tasks: &'a [AutonomyTask],
    selected: &[String],
) -> Result<Vec<&'a AutonomyTask>, String> {
    if selected.is_empty() {
        return Ok(tasks.iter().collect());
    }
    let known: BTreeSet<_> = tasks.iter().map(|task| task.id.as_str()).collect();
    let unknown: Vec<_> = selected
        .iter()
        .filter(|id| !known.contains(id.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "unknown autonomy task id(s): {}",
            unknown.join(", ")
        ));
    }
    let requested: BTreeSet<_> = selected.iter().map(String::as_str).collect();
    Ok(tasks
        .iter()
        .filter(|task| requested.contains(task.id.as_str()))
        .collect())
}

fn select_variants(selected: &[AutonomyVariantArg]) -> Vec<AutonomyVariantArg> {
    let mut variants = if selected.is_empty() {
        vec![
            AutonomyVariantArg::Current,
            AutonomyVariantArg::Recovery,
            AutonomyVariantArg::RepositoryBrief,
        ]
    } else {
        selected.to_vec()
    };
    variants.sort();
    variants.dedup();
    variants
}

fn expected_terminals(task: &AutonomyTask, variant: AutonomyVariantArg) -> Vec<TerminalOutcome> {
    if variant == AutonomyVariantArg::Current {
        vec![task.terminal.current]
    } else {
        task.terminal.resumable_outcomes.clone()
    }
}

fn injection_for(task: &AutonomyTask, segment: u32) -> Option<&ferric_bench::RecoveryInjection> {
    task.recovery
        .as_ref()?
        .injections
        .iter()
        .find(|injection| injection.segment == segment)
}

fn materialize_task(workspace: &Path, task: &AutonomyTask) -> Result<(), String> {
    for (relative, content) in &task.setup_files {
        let path = workspace.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .map_err(|error| format!("write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn materialized_tree_sha256(workspace: &Path) -> Result<String, String> {
    fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        let mut entries = std::fs::read_dir(directory)
            .map_err(|error| format!("read initial tree {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read initial tree entry: {error}"))?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let file_type = entry
                .file_type()
                .map_err(|error| format!("inspect initial tree entry: {error}"))?;
            if file_type.is_dir() {
                collect_files(&entry.path(), files)?;
            } else if file_type.is_file() {
                files.push(entry.path());
            } else {
                return Err(format!(
                    "initial tree contains unsupported non-file entry {}",
                    entry.path().display()
                ));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    collect_files(workspace, &mut files)?;
    let mut files = files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(workspace)
                .map_err(|error| format!("relativize initial tree path: {error}"))?
                .to_string_lossy()
                .replace('\\', "/");
            Ok((relative, path))
        })
        .collect::<Result<Vec<_>, String>>()?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut framed = Vec::new();
    for (relative, path) in files {
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("read initial tree file {}: {error}", path.display()))?;
        framed.extend_from_slice(&(relative.len() as u64).to_le_bytes());
        framed.extend_from_slice(relative.as_bytes());
        framed.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        framed.extend_from_slice(&bytes);
    }
    Ok(ferric_bench::sha256_bytes(&framed))
}

fn write_named_checks(path: &Path, checks: &[AutonomyCheck], python: &Path) -> Result<(), String> {
    let mut text = String::new();
    for check in checks {
        if check.expected_exit != 0 || check.stdout_regex.is_some() || check.stderr_regex.is_some()
        {
            return Err(format!(
                "check `{}` cannot be used as an in-loop completion gate: only exit 0 without output regex is supported",
                check.name
            ));
        }
        let program = if check.argv.first().map(String::as_str) == Some("{python}") {
            python.display().to_string()
        } else {
            check.argv.first().cloned().unwrap_or_default()
        };
        text.push_str("[[check]]\nname = ");
        text.push_str(&toml_string(&check.name));
        text.push_str("\nprogram = ");
        text.push_str(&toml_string(&program));
        text.push_str("\nargs = [");
        for (index, argument) in check.argv.iter().skip(1).enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            text.push_str(&toml_string(argument));
        }
        text.push_str(&format!(
            "]\ntimeout_s = {}\noutput_limit = 16000\n\n",
            check.timeout_s
        ));
    }
    std::fs::write(path, text).map_err(|error| format!("write {}: {error}", path.display()))
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn preflight_autonomy_checks(tasks: &[&AutonomyTask], python: &Path) -> Result<(), String> {
    let specs: Vec<_> = tasks
        .iter()
        .map(|task| autonomy_bench_spec(task, 1))
        .collect();
    preflight_command_checks(&specs, python)
}

#[cfg(feature = "backend-openai")]
fn preflight_openai_server(api_base: &str, model: &str) -> Result<(), String> {
    const RESPONSE_LIMIT: usize = 1024 * 1024;
    let url = format!("{}/models", api_base.trim_end_matches('/'));
    let runtime =
        tokio::runtime::Runtime::new().map_err(|error| format!("tokio runtime: {error}"))?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| format!("build HTTP client: {error}"))?;
        let mut request = client.get(&url);
        if let Ok(key) = std::env::var("OPENAI_API_KEY")
            && !key.trim().is_empty()
        {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("GET {url}: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("GET {url} returned HTTP {}", response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > RESPONSE_LIMIT as u64)
        {
            return Err(format!("GET {url} response exceeds {RESPONSE_LIMIT} bytes"));
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| format!("read {url}: {error}"))?;
        if body.len() > RESPONSE_LIMIT {
            return Err(format!("GET {url} response exceeds {RESPONSE_LIMIT} bytes"));
        }
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|error| format!("parse {url} response: {error}"))?;
        let ids = model_ids(&value);
        if ids.iter().any(|id| id == model) {
            Ok(())
        } else if ids.is_empty() {
            Err(format!("GET {url} returned no model identifiers"))
        } else {
            Err(format!(
                "model {model:?} is not exposed by {url}; available: {}",
                ids.join(", ")
            ))
        }
    })
}

#[cfg(not(feature = "backend-openai"))]
fn preflight_openai_server(_api_base: &str, _model: &str) -> Result<(), String> {
    Err(crate::backend::BACKEND_FEATURE_MISSING.to_string())
}

#[cfg(any(feature = "backend-openai", test))]
fn model_ids(value: &serde_json::Value) -> Vec<String> {
    let mut ids = BTreeSet::new();
    if let Some(models) = value.get("data").and_then(serde_json::Value::as_array) {
        ids.extend(models.iter().filter_map(|model| {
            model
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        }));
    }
    if let Some(models) = value.get("models").and_then(serde_json::Value::as_array) {
        ids.extend(models.iter().filter_map(|model| {
            model
                .get("model")
                .or_else(|| model.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        }));
    }
    ids.into_iter().collect()
}

#[derive(Debug)]
struct TraceAnalysis {
    session: String,
    resumed_from: Option<String>,
    terminal: Option<String>,
    questions: Vec<String>,
    resume_prompts: Vec<String>,
    mutation_before_question: bool,
    metrics: AutonomyTraceMetrics,
    tool_turns: Vec<u32>,
    failed_diagnostic_fingerprints: BTreeSet<String>,
}

#[derive(Debug)]
struct TraceTailTurn {
    empty_pre_response_prefix: bool,
}

impl TraceTailTurn {
    fn is_empty_pre_response_prefix(&self) -> bool {
        self.empty_pre_response_prefix
    }
}

fn observe_trace_tail(event: &Event, tail: &mut Option<TraceTailTurn>) {
    match event {
        Event::TurnStart { .. } => {
            *tail = Some(TraceTailTurn {
                empty_pre_response_prefix: true,
            });
        }
        Event::PromptAssembled { .. }
        | Event::ConstraintApplied { .. }
        | Event::HistoryCompacted { .. } => {}
        Event::TurnCommitted { .. } | Event::SessionEnd { .. } => *tail = None,
        _ => {
            if let Some(tail) = tail {
                tail.empty_pre_response_prefix = false;
            }
        }
    }
}

#[cfg(test)]
fn analyze_trace(
    path: &Path,
    expected_workspace: &Path,
    expected_protocol: ActionProtocol,
    expected_max_turns: u32,
    expected_harness_policy: HarnessPolicy,
) -> Result<TraceAnalysis, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("read retained trace {}: {error}", path.display()))?;
    analyze_trace_bytes(
        &bytes,
        expected_workspace,
        expected_protocol,
        expected_max_turns,
        expected_harness_policy,
    )
}

fn analyze_trace_bytes(
    bytes: &[u8],
    expected_workspace: &Path,
    expected_protocol: ActionProtocol,
    expected_max_turns: u32,
    expected_harness_policy: HarnessPolicy,
) -> Result<TraceAnalysis, String> {
    let expected_workspace = std::fs::canonicalize(expected_workspace)
        .map_err(|error| format!("canonicalize expected workspace: {error}"))?;
    let mut structure = TraceStructure::new();
    let mut terminal = None;
    let mut questions = Vec::new();
    let mut resume_prompts = Vec::new();
    let mut resumed_from = None;
    let mut saw_mutation = false;
    let mut mutation_before_question = false;
    let mut metrics = AutonomyTraceMetrics::default();
    let mut current_turn = None;
    let mut tool_turns = Vec::new();
    let mut failed_diagnostic_fingerprints = BTreeSet::new();
    let mut expected_session: Option<String> = None;
    let mut expected_seq = 0_u64;
    let mut saw_session_start = false;
    let mut saw_policy_selected = false;
    let mut trace_tail = None;
    let reader = std::io::BufReader::new(std::io::Cursor::new(bytes));
    for (line_index, line) in reader.lines().enumerate() {
        let line =
            line.map_err(|error| format!("read retained trace line {}: {error}", line_index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let raw: serde_json::Value = serde_json::from_str(&line)
            .map_err(|error| format!("parse retained trace line {}: {error}", line_index + 1))?;
        let version = raw
            .get("v")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| format!("retained trace line {} has no valid schema", line_index + 1))?;
        raw.get("ts_ms")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                format!(
                    "retained trace line {} has no valid timestamp",
                    line_index + 1
                )
            })?;
        let record_session = raw
            .get("session")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("retained trace line {} has no session", line_index + 1))?
            .to_string();
        let sequence = raw
            .get("seq")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("retained trace line {} has no sequence", line_index + 1))?;
        let event: Event = serde_json::from_value(
            raw.get("event")
                .cloned()
                .ok_or_else(|| format!("retained trace line {} has no event", line_index + 1))?,
        )
        .map_err(|_| format!("unknown trace event at sequence {sequence}"))?;
        if version != TRACE_SCHEMA_VERSION {
            return Err(format!("unsupported trace schema {version}"));
        }
        if let Some(session) = &expected_session {
            if session != &record_session {
                return Err("trace mixes session identifiers".to_string());
            }
        } else {
            expected_session = Some(record_session);
        }
        if sequence != expected_seq {
            return Err(format!(
                "trace sequence gap: expected {expected_seq}, found {}",
                sequence
            ));
        }
        expected_seq = expected_seq.saturating_add(1);
        structure.observe(&event)?;
        observe_trace_tail(&event, &mut trace_tail);
        match &event {
            Event::SessionStart {
                workspace,
                resumed_from: source,
                ..
            } => {
                if saw_session_start || sequence != 0 {
                    return Err(
                        "trace must contain exactly one sequence-zero session_start".to_string()
                    );
                }
                let recorded_workspace = std::fs::canonicalize(workspace).map_err(|error| {
                    format!("canonicalize recorded session workspace {workspace:?}: {error}")
                })?;
                if recorded_workspace != expected_workspace {
                    return Err(format!(
                        "trace workspace {} differs from episode workspace {}",
                        recorded_workspace.display(),
                        expected_workspace.display()
                    ));
                }
                saw_session_start = true;
                resumed_from = source.clone();
            }
            Event::PolicySelected {
                protocol,
                harness_policy,
                max_turns,
                ..
            } => {
                if !saw_session_start || saw_policy_selected || sequence != 1 {
                    return Err(
                        "trace must contain exactly one policy_selected after session_start"
                            .to_string(),
                    );
                }
                if *protocol != expected_protocol
                    || *max_turns != expected_max_turns
                    || *harness_policy != expected_harness_policy
                {
                    return Err(format!(
                        "trace policy differs from episode request: protocol {:?}/{:?}, harness {}/{}, max_turns {}/{}",
                        protocol,
                        expected_protocol,
                        harness_policy,
                        expected_harness_policy,
                        max_turns,
                        expected_max_turns
                    ));
                }
                saw_policy_selected = true;
            }
            Event::ResumePrompt { user, .. } => resume_prompts.push(user.clone()),
            Event::TurnStart { turn } => {
                if !saw_policy_selected {
                    return Err("trace starts a turn before policy_selected".to_string());
                }
                metrics.turns = metrics.turns.saturating_add(1);
                current_turn = Some(*turn);
            }
            Event::TurnEnd {
                input_tokens,
                output_tokens,
                truncated,
                ..
            } => {
                metrics.input_tokens = metrics
                    .input_tokens
                    .saturating_add(u64::from(input_tokens.unwrap_or(0)));
                metrics.output_tokens = metrics
                    .output_tokens
                    .saturating_add(u64::from(output_tokens.unwrap_or(0)));
                metrics.truncations += u32::from(*truncated);
            }
            Event::ToolCall { name, args, .. } => {
                metrics.tool_calls = metrics.tool_calls.saturating_add(1);
                metrics.tools_called.push(name.clone());
                tool_turns.push(current_turn.unwrap_or_default());
                if name == "request_user_input"
                    && let Some(question) = args.get("question").and_then(|value| value.as_str())
                {
                    mutation_before_question |= saw_mutation;
                    questions.push(question.to_string());
                }
            }
            Event::ToolResult { is_error, .. } => {
                metrics.tool_errors += u32::from(*is_error);
            }
            Event::WorkspaceMutation { .. } => {
                metrics.mutations += 1;
                saw_mutation = true;
            }
            Event::VerificationCheckPassed { .. } => metrics.verification_passes += 1,
            Event::CompletionGate { decision, .. } if decision == "passed" => {
                metrics.completion_gates_passed += 1
            }
            Event::CompletionGate { decision, .. } if decision == "blocked" => {
                metrics.completion_gates_blocked += 1
            }
            Event::RepetitionGuard { action } if action == "stopped" => {
                metrics.repetition_guard_stops += 1
            }
            Event::NoProgressGuard { action } if action == "stopped" => {
                metrics.no_progress_guard_stops += 1
            }
            Event::FailureGuard { action } if action == "stopped" => {
                metrics.failure_guard_stops += 1
            }
            Event::OscillationGuard { action } if action == "stopped" => {
                metrics.oscillation_guard_stops += 1
            }
            Event::HistoryCompacted { .. } => metrics.history_compactions += 1,
            Event::ObservationRecorded { observation, .. } => {
                record_observation_metrics(&mut metrics, &observation.detail);
            }
            Event::ControllerBlocked { block, .. } => {
                record_controller_block(&mut metrics, block.reason);
            }
            Event::WorkspaceEffectRecorded { effect, .. } => {
                metrics.workspace_effects_recorded += 1;
                metrics.workspace_effect_paths = metrics
                    .workspace_effect_paths
                    .saturating_add(effect.effects.len() as u32);
            }
            Event::VerificationCheckRecorded { check, .. } => {
                record_verification_check(&mut metrics, &mut failed_diagnostic_fingerprints, check);
            }
            Event::ControllerCheckpoint { .. } => metrics.controller_checkpoints += 1,
            Event::RecoveryPacketInjected { .. } => metrics.recovery_packets_injected += 1,
            Event::SessionEnd { reason } => {
                terminal = Some(reason.clone());
                if reason == "provider_error" {
                    metrics.provider_error_stops += 1;
                }
            }
            _ => {}
        }
    }
    structure.finish()?;
    if let Some(tail) = trace_tail
        && tail.is_empty_pre_response_prefix()
    {
        // TurnStart is counted eagerly for complete traces. An interrupted
        // request-side prefix has no model response and is not a completed
        // evaluation turn.
        metrics.turns = metrics.turns.saturating_sub(1);
    }
    if !saw_session_start || !saw_policy_selected {
        return Err("trace is missing session_start or policy_selected".to_string());
    }
    metrics.distinct_failed_diagnostic_fingerprints = failed_diagnostic_fingerprints.len() as u32;
    Ok(TraceAnalysis {
        session: expected_session.ok_or_else(|| "trace contains no events".to_string())?,
        resumed_from,
        terminal,
        questions,
        resume_prompts,
        mutation_before_question,
        metrics,
        tool_turns,
        failed_diagnostic_fingerprints,
    })
}

fn record_observation_metrics(metrics: &mut AutonomyTraceMetrics, detail: &ObservationDetailV1) {
    metrics.observations_recorded += 1;
    match detail {
        ObservationDetailV1::File(_) => metrics.file_observations += 1,
        ObservationDetailV1::Search(_) => metrics.search_observations += 1,
        ObservationDetailV1::Find(_) => metrics.find_observations += 1,
    }
}

fn record_controller_block(metrics: &mut AutonomyTraceMetrics, reason: ControllerBlockReason) {
    metrics.controller_blocks += 1;
    match reason {
        ControllerBlockReason::BlindMutation => metrics.blind_mutation_blocks += 1,
        ControllerBlockReason::SameTurnObservation => metrics.same_turn_observation_blocks += 1,
        ControllerBlockReason::StaleObservation => metrics.stale_observation_blocks += 1,
        ControllerBlockReason::UnsupportedMutation => metrics.unsupported_mutation_blocks += 1,
        ControllerBlockReason::RepairInspectionRequired => metrics.repair_inspection_blocks += 1,
        ControllerBlockReason::NoEffect => metrics.no_effect_blocks += 1,
        ControllerBlockReason::SyntaxRegression => metrics.syntax_regression_blocks += 1,
        ControllerBlockReason::RepeatedCheck => metrics.repeated_check_blocks += 1,
    }
}

fn record_verification_check(
    metrics: &mut AutonomyTraceMetrics,
    failed_diagnostic_fingerprints: &mut BTreeSet<String>,
    check: &VerificationCheckV1,
) {
    metrics.verification_checks_recorded += 1;
    metrics.verification_repair_attempts += u32::from(check.attempt > 1);
    match check.outcome {
        VerificationOutcome::Passed => metrics.verification_checks_passed += 1,
        VerificationOutcome::Failed => {
            metrics.verification_checks_failed += 1;
            if let Some(diagnostic) = &check.diagnostic_sha256 {
                failed_diagnostic_fingerprints.insert(diagnostic.clone());
            }
        }
    }
}

fn aggregate_metrics(analyses: &[TraceAnalysis]) -> AutonomyTraceMetrics {
    let mut aggregate = AutonomyTraceMetrics::default();
    let mut tool_turns = Vec::new();
    let mut failed_diagnostic_fingerprints = BTreeSet::new();
    for analysis in analyses {
        let metrics = &analysis.metrics;
        aggregate.turns = aggregate.turns.saturating_add(metrics.turns);
        aggregate.input_tokens = aggregate.input_tokens.saturating_add(metrics.input_tokens);
        aggregate.output_tokens = aggregate
            .output_tokens
            .saturating_add(metrics.output_tokens);
        aggregate.tool_calls = aggregate.tool_calls.saturating_add(metrics.tool_calls);
        aggregate.tool_errors = aggregate.tool_errors.saturating_add(metrics.tool_errors);
        aggregate.mutations = aggregate.mutations.saturating_add(metrics.mutations);
        aggregate.verification_passes = aggregate
            .verification_passes
            .saturating_add(metrics.verification_passes);
        aggregate.completion_gates_passed = aggregate
            .completion_gates_passed
            .saturating_add(metrics.completion_gates_passed);
        aggregate.completion_gates_blocked = aggregate
            .completion_gates_blocked
            .saturating_add(metrics.completion_gates_blocked);
        aggregate.repetition_guard_stops += metrics.repetition_guard_stops;
        aggregate.no_progress_guard_stops += metrics.no_progress_guard_stops;
        aggregate.failure_guard_stops += metrics.failure_guard_stops;
        aggregate.oscillation_guard_stops += metrics.oscillation_guard_stops;
        aggregate.truncations += metrics.truncations;
        aggregate.provider_error_stops += metrics.provider_error_stops;
        aggregate.history_compactions += metrics.history_compactions;
        aggregate.observations_recorded += metrics.observations_recorded;
        aggregate.file_observations += metrics.file_observations;
        aggregate.search_observations += metrics.search_observations;
        aggregate.find_observations += metrics.find_observations;
        aggregate.controller_blocks += metrics.controller_blocks;
        aggregate.blind_mutation_blocks += metrics.blind_mutation_blocks;
        aggregate.same_turn_observation_blocks += metrics.same_turn_observation_blocks;
        aggregate.stale_observation_blocks += metrics.stale_observation_blocks;
        aggregate.unsupported_mutation_blocks += metrics.unsupported_mutation_blocks;
        aggregate.repair_inspection_blocks += metrics.repair_inspection_blocks;
        aggregate.no_effect_blocks += metrics.no_effect_blocks;
        aggregate.syntax_regression_blocks += metrics.syntax_regression_blocks;
        aggregate.repeated_check_blocks += metrics.repeated_check_blocks;
        aggregate.workspace_effects_recorded += metrics.workspace_effects_recorded;
        aggregate.workspace_effect_paths += metrics.workspace_effect_paths;
        aggregate.verification_checks_recorded += metrics.verification_checks_recorded;
        aggregate.verification_checks_passed += metrics.verification_checks_passed;
        aggregate.verification_checks_failed += metrics.verification_checks_failed;
        aggregate.verification_repair_attempts += metrics.verification_repair_attempts;
        aggregate.controller_checkpoints += metrics.controller_checkpoints;
        aggregate.recovery_packets_injected += metrics.recovery_packets_injected;
        failed_diagnostic_fingerprints
            .extend(analysis.failed_diagnostic_fingerprints.iter().cloned());
        aggregate.tools_called.extend(metrics.tools_called.clone());
        tool_turns.extend(&analysis.tool_turns);
    }
    if !tool_turns.is_empty() {
        let threshold = (tool_turns.len() * 4).div_ceil(5);
        aggregate.horizon_80_turn = tool_turns.get(threshold.saturating_sub(1)).copied();
    }
    aggregate.distinct_failed_diagnostic_fingerprints = failed_diagnostic_fingerprints.len() as u32;
    aggregate
}

fn terminal_matches(
    expected: TerminalOutcome,
    observed: Option<&str>,
    exit_code: Option<i32>,
    timed_out: bool,
) -> bool {
    if timed_out {
        return false;
    }
    match expected {
        TerminalOutcome::Completed => exit_code == Some(0) && observed == Some("task_complete"),
        TerminalOutcome::NeedsInput => {
            exit_code.is_some_and(|code| code != 0) && observed == Some("needs_input")
        }
        TerminalOutcome::Paused => {
            exit_code.is_some_and(|code| code != 0) && observed.is_some_and(is_pause_terminal)
        }
        TerminalOutcome::ResumeRejected => {
            exit_code.is_some_and(|code| code != 0) && observed.is_none()
        }
    }
}

fn injection_matches(kind: Option<RecoveryInjectionKind>, observed: Option<&str>) -> bool {
    match kind {
        None => true,
        Some(kind) => kind.expected_stop_reason() == observed,
    }
}

fn is_pause_terminal(reason: &str) -> bool {
    matches!(
        reason,
        "max_turns"
            | "repetition_guard"
            | "no_progress"
            | "repeated_failure"
            | "oscillation"
            | "provider_error"
            | "empty_completion"
            | "truncated_action"
            | "interrupted"
            | "hook_failed"
    )
}

fn terminal_label(outcome: TerminalOutcome) -> &'static str {
    match outcome {
        TerminalOutcome::Completed => "completed",
        TerminalOutcome::NeedsInput => "needs_input",
        TerminalOutcome::Paused => "paused",
        TerminalOutcome::ResumeRejected => "resume_rejected",
    }
}

fn category_label(category: AutonomyCategory) -> &'static str {
    match category {
        AutonomyCategory::Ambiguity => "ambiguity",
        AutonomyCategory::Recovery => "recovery",
        AutonomyCategory::LongHorizon => "long_horizon",
    }
}

fn question_matches_terms(questions: &[&str], expected_terms: &[String]) -> bool {
    expected_terms.iter().any(|term| {
        let term = term.to_ascii_lowercase();
        questions
            .iter()
            .any(|question| question.to_ascii_lowercase().contains(&term))
    })
}

fn retain_autonomy_trace(
    source: &Path,
    results_dir: &Path,
    run_id: &str,
    key: &RetainedTraceKey<'_>,
) -> std::io::Result<RetainedTrace> {
    let relative = format!(
        "traces/{run_id}/trial-{:03}-{}-{}-{}-{}-s{:02}.jsonl",
        key.trial,
        key.task_id,
        key.variant,
        key.coordinate.arm.label(),
        key.coordinate.harness_policy.label(),
        key.segment,
    );
    let destination = results_dir.join(&relative);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Snapshot the child trace once. The digest and structural analysis both
    // consume this exact byte vector; later source-file changes cannot alter
    // the evidence represented by the row.
    let bytes = std::fs::read(source)?;
    let digest = ferric_bench::sha256_bytes(&bytes);
    let mut output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&destination)?;
    output.write_all(&bytes)?;
    output.sync_all()?;
    let mut permissions = output.metadata()?.permissions();
    permissions.set_readonly(true);
    drop(output);
    if std::fs::read(&destination)? != bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "retained trace bytes differ from the captured snapshot",
        ));
    }
    std::fs::set_permissions(&destination, permissions)?;
    Ok(RetainedTrace {
        relative_path: relative,
        absolute_path: destination,
        sha256: digest,
        bytes,
    })
}

fn run_workspace_mismatch_probe(
    episode: &EpisodeCoordinate,
    trace: &Path,
    profile_dir: &Path,
    checks_file: &Path,
    task: &AutonomyTask,
    timeout: Duration,
) -> Result<ResumeProbeResult, String> {
    let other = tempfile::tempdir().map_err(|error| format!("create mismatch probe: {error}"))?;
    run_refusal_probe(
        "workspace_mismatch",
        episode,
        trace,
        other.path(),
        profile_dir,
        checks_file,
        task,
        "workspace",
        timeout,
    )
}

fn run_completed_probe(
    episode: &EpisodeCoordinate,
    trace: &Path,
    workspace: &Path,
    profile_dir: &Path,
    checks_file: &Path,
    task: &AutonomyTask,
    timeout: Duration,
) -> Result<ResumeProbeResult, String> {
    run_refusal_probe(
        "completed_session",
        episode,
        trace,
        workspace,
        profile_dir,
        checks_file,
        task,
        "completed",
        timeout,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_refusal_probe(
    mode: &str,
    episode: &EpisodeCoordinate,
    trace: &Path,
    workspace: &Path,
    profile_dir: &Path,
    checks_file: &Path,
    task: &AutonomyTask,
    expected_stderr: &str,
    timeout: Duration,
) -> Result<ResumeProbeResult, String> {
    let request = QuerySegmentRequest {
        workspace,
        profile_dir,
        checks_file: Some(checks_file),
        prompt: None,
        resume: Some(trace),
        answer: None,
        max_turns: task.max_turns,
        timeout,
        api_base_override: None,
        harness_policy: episode.child_policy_flag,
    };
    let process = run_verified_query_segment(episode, &request, &format!("{mode} refusal probe"))?;
    if process.timed_out {
        return Err(format!("{mode} refusal probe timed out"));
    }
    if let Some(error) = &process.trace_discovery_error {
        return Err(format!("{mode} refusal probe trace discovery: {error}"));
    }
    let rejected = process.exit_code.is_some_and(|code| code != 0)
        && !process.timed_out
        && process.trace_path.is_none()
        && process.trace_discovery_error.is_none()
        && process
            .stderr_tail
            .to_ascii_lowercase()
            .contains(expected_stderr);
    Ok(ResumeProbeResult {
        mode: mode.to_string(),
        attempted: true,
        rejected,
        exit_code: process.exit_code,
        stderr_tail: trim_tail(&process.stderr_tail),
    })
}

fn provenance(
    invocation: &Invocation,
    args: &AutonomyArgs,
    variant: &str,
    frozen_binary: Option<&BinaryProvenance>,
) -> RunProvenance {
    let binary = frozen_binary.cloned().unwrap_or_else(|| {
        let binary_path = std::fs::canonicalize(&invocation.ferric_bin)
            .unwrap_or_else(|_| invocation.ferric_bin.clone());
        let metadata = std::fs::metadata(&binary_path).ok();
        BinaryProvenance {
            path: binary_path.display().to_string(),
            size_bytes: metadata.as_ref().map(std::fs::Metadata::len),
            modified_at_unix_ms: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_unix_ms),
            sha256: ferric_bench::sha256_file(&binary_path).ok(),
        }
    });
    let child_matches_runner = std::env::current_exe()
        .ok()
        .and_then(|path| ferric_bench::sha256_file(&path).ok())
        .zip(binary.sha256.as_ref())
        .is_some_and(|(runner, child)| &runner == child);
    RunProvenance {
        ferric_version: if child_matches_runner {
            env!("CARGO_PKG_VERSION").to_string()
        } else {
            "unknown".to_string()
        },
        git_commit: child_matches_runner
            .then(|| {
                option_env!("FERRIC_GIT_COMMIT")
                    .or(option_env!("VERGEN_GIT_SHA"))
                    .or(option_env!("GITHUB_SHA"))
                    .map(str::to_string)
            })
            .flatten(),
        binary,
        model: ModelProvenance {
            backend: "openai".to_string(),
            model: args.model.clone(),
            api_base: invocation
                .openai
                .as_ref()
                .and_then(|openai| openai.api_base.clone()),
            params_b: args.params_b,
            ctx: args.ctx,
            sha256: args.model_sha256.clone(),
        },
        protocol: ferric_core::protocol_key(invocation.protocol),
        variant: variant.to_string(),
        python_bin: args.python_bin.display().to_string(),
    }
}

fn pass_label(value: bool) -> &'static str {
    if value { "PASS" } else { "FAIL" }
}

fn parse_sha256(value: &str) -> Result<String, String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("SHA-256 must contain exactly 64 hexadecimal characters".to_string())
    }
}

fn remaining_before(
    deadline: Instant,
    task: &AutonomyTask,
    operation: &str,
) -> Result<Duration, String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(format!(
            "{} exhausted its {}-second episode deadline before {operation}",
            task.id, task.timeout_s
        ))
    } else {
        Ok(remaining)
    }
}

fn trim_tail(value: &str) -> String {
    if value.len() <= STDERR_TAIL_BYTES {
        return value.to_string();
    }
    let mut start = value.len() - STDERR_TAIL_BYTES;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value[start..].to_string()
}

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
static WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_run_id(started_at_unix_ms: u64) -> String {
    let sequence = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "autonomy-{started_at_unix_ms}-{}-{sequence}",
        std::process::id()
    )
}

fn now_unix_ms() -> u64 {
    system_time_unix_ms(SystemTime::now()).unwrap_or_default()
}

fn system_time_unix_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn evidence_analysis_prefix(workspace: &Path) -> Vec<Event> {
        let controller =
            ferric_loop::ControllerState::new(HarnessPolicy::Evidence, Vec::<String>::new())
                .unwrap();
        vec![
            Event::SessionStart {
                workspace: std::fs::canonicalize(workspace)
                    .unwrap()
                    .display()
                    .to_string(),
                resumed_from: None,
            },
            Event::PolicySelected {
                tier: ferric_core::Tier::Small,
                protocol: ActionProtocol::ConstrainedJson,
                harness_policy: HarnessPolicy::Evidence,
                max_turns: 7,
                max_tools: 8,
                prompt_budget_tokens: 4096,
                max_output_tokens: 1024,
                truncation_limit: 4000,
                tier_source: "params".to_string(),
            },
            Event::SessionPrompt {
                system: "system".to_string(),
                user: "task".to_string(),
                media: Vec::new(),
            },
            Event::ControllerCheckpoint {
                state: controller.checkpoint(),
            },
        ]
    }

    fn write_analysis_trace(path: &Path, events: impl IntoIterator<Item = Event>) {
        let mut sink = ferric_trace::JsonlSink::create_new(path, "analysis-test").unwrap();
        for event in events {
            sink.write_event(event).unwrap();
        }
    }

    #[test]
    fn cli_parser_keeps_paired_binary_coordinates_distinct() {
        let cli = crate::Cli::try_parse_from([
            "ferric",
            "bench",
            "autonomy",
            "--paired",
            "--control-bin",
            "control-ferric.exe",
            "--control-sha256",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "--candidate-bin",
            "candidate-ferric.exe",
            "--model-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--ctx",
            "8192",
            "--task",
            "H01",
        ])
        .unwrap();
        let crate::Command::Bench {
            command: crate::BenchCommand::Autonomy(mut args),
        } = cli.command
        else {
            panic!("expected autonomy command");
        };
        assert!(args.paired);
        assert_eq!(args.control_bin, Some(PathBuf::from("control-ferric.exe")));
        assert_eq!(
            args.candidate_bin,
            Some(PathBuf::from("candidate-ferric.exe"))
        );
        assert!(args.ferric_bin.is_none());
        assert_eq!(args.task, ["H01"]);
        assert_eq!(args.control_sha256, Some("b".repeat(64)));
        validate_binary_mode(&args).unwrap();
        args.control_sha256 = None;
        assert!(
            validate_binary_mode(&args)
                .unwrap_err()
                .contains("--control-sha256")
        );
        args.control_sha256 = Some("b".repeat(64));
        args.model_sha256 = None;
        assert!(
            validate_binary_mode(&args)
                .unwrap_err()
                .contains("--model-sha256")
        );
        args.model_sha256 = Some("a".repeat(64));
        args.ferric_bin = Some(PathBuf::from("third.exe"));
        assert!(
            validate_binary_mode(&args)
                .unwrap_err()
                .contains("conflicts")
        );
    }

    #[test]
    fn single_policy_parser_coordinates_and_strict_gate_are_explicit() {
        let parse = |policy: Option<&str>| {
            let mut argv = vec!["ferric", "bench", "autonomy"];
            if let Some(policy) = policy {
                argv.extend(["--harness-policy", policy]);
            }
            crate::Cli::try_parse_from(argv)
        };

        let crate::Command::Bench {
            command: crate::BenchCommand::Autonomy(default),
        } = parse(None).unwrap().command
        else {
            panic!("expected autonomy command");
        };
        assert_eq!(
            single_evaluation_coordinate(&default),
            (AutonomyEvaluationCoordinate::single_legacy(), None)
        );
        assert!(!strict_managed_mode(&default));

        let crate::Command::Bench {
            command: crate::BenchCommand::Autonomy(legacy),
        } = parse(Some("legacy")).unwrap().command
        else {
            panic!("expected autonomy command");
        };
        assert_eq!(
            single_evaluation_coordinate(&legacy),
            (
                AutonomyEvaluationCoordinate::single_legacy(),
                Some(HarnessPolicy::Legacy)
            )
        );

        let crate::Command::Bench {
            command: crate::BenchCommand::Autonomy(mut evidence),
        } = parse(Some("evidence")).unwrap().command
        else {
            panic!("expected autonomy command");
        };
        assert_eq!(
            single_evaluation_coordinate(&evidence),
            (
                AutonomyEvaluationCoordinate {
                    arm: AutonomyArm::Single,
                    harness_policy: HarnessPolicy::Evidence,
                },
                Some(HarnessPolicy::Evidence)
            )
        );
        assert!(strict_managed_mode(&evidence));
        assert!(
            validate_binary_mode(&evidence)
                .unwrap_err()
                .contains("--model-sha256")
        );
        evidence.model_sha256 = Some("a".repeat(64));
        assert!(
            validate_binary_mode(&evidence)
                .unwrap_err()
                .contains("--ctx 8192")
        );
        evidence.ctx = STRICT_AUTONOMY_CONTEXT;
        validate_binary_mode(&evidence).unwrap();
        assert!(parse(Some("evidence-planner")).is_err());
    }

    #[test]
    fn external_child_does_not_inherit_runner_build_metadata() {
        let cli = crate::Cli::try_parse_from(["ferric", "bench", "autonomy"]).unwrap();
        let crate::Command::Bench {
            command: crate::BenchCommand::Autonomy(args),
        } = cli.command
        else {
            panic!("expected autonomy command");
        };
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("ferric-child.exe");
        std::fs::write(&child, b"external child bytes").unwrap();
        let digest = ferric_bench::sha256_file(&child).unwrap();
        let binary = BinaryProvenance {
            path: child.display().to_string(),
            size_bytes: Some(std::fs::metadata(&child).unwrap().len()),
            modified_at_unix_ms: None,
            sha256: Some(digest),
        };
        let invocation = Invocation {
            ferric_bin: child,
            protocol: ActionProtocol::ConstrainedJson,
            openai: None,
            prompts_dir: None,
            keep_workspace: false,
        };
        let provenance = provenance(&invocation, &args, "current", Some(&binary));
        assert_eq!(provenance.ferric_version, "unknown");
        assert!(provenance.git_commit.is_none());
    }

    #[test]
    fn selected_variants_are_deduplicated_and_stable() {
        assert_eq!(
            select_variants(&[
                AutonomyVariantArg::RepositoryBrief,
                AutonomyVariantArg::Current,
                AutonomyVariantArg::Current,
            ]),
            vec![
                AutonomyVariantArg::Current,
                AutonomyVariantArg::RepositoryBrief
            ]
        );
    }

    #[test]
    fn paired_schedule_is_adjacent_deterministic_and_counterbalanced() {
        let first = episode_coordinate_order("H01", AutonomyVariantArg::Recovery, 1);
        let repeat = episode_coordinate_order("H01", AutonomyVariantArg::Recovery, 1);
        let second = episode_coordinate_order("H01", AutonomyVariantArg::Recovery, 2);
        assert_eq!(first, repeat);
        assert!(matches!(first, [0, 1] | [1, 0]));
        assert_eq!(second, [first[1], first[0]]);
        assert_eq!(
            pair_id("H01", AutonomyVariantArg::Recovery, 2),
            "trial-002-H01-recovery"
        );
    }

    #[test]
    fn frozen_pair_rejects_nonfiles_and_equal_digests() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join("control.exe");
        let candidate = dir.path().join("candidate.exe");
        std::fs::write(&control, b"same").unwrap();
        std::fs::write(&candidate, b"same").unwrap();
        let expected = ferric_bench::sha256_file(&control).unwrap();
        let error = freeze_paired_binaries(
            &dir.path().join("results"),
            "run",
            &control,
            &candidate,
            &expected,
        )
        .err()
        .unwrap();
        assert!(error.contains("same SHA-256"));

        std::fs::write(&candidate, b"different").unwrap();
        let error = freeze_paired_binaries(
            &dir.path().join("other-results"),
            "run",
            &control,
            &candidate,
            &"0".repeat(64),
        )
        .err()
        .unwrap();
        assert!(error.contains("control binary SHA-256 mismatch"));

        let error = checked_binary(dir.path(), "control").unwrap_err();
        assert!(error.contains("not a regular file"));
    }

    #[test]
    fn frozen_pair_uses_verified_read_only_copies() {
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join("control.exe");
        let candidate = dir.path().join("candidate.exe");
        std::fs::write(&control, b"control").unwrap();
        std::fs::write(&candidate, b"candidate").unwrap();
        let expected = ferric_bench::sha256_file(&control).unwrap();
        let frozen = freeze_paired_binaries(
            &dir.path().join("results"),
            "run",
            &control,
            &candidate,
            &expected,
        )
        .unwrap();
        assert_eq!(
            ferric_bench::sha256_file(&frozen.control.path).unwrap(),
            ferric_bench::sha256_file(&control).unwrap()
        );
        assert_eq!(
            ferric_bench::sha256_file(&frozen.candidate.path).unwrap(),
            ferric_bench::sha256_file(&candidate).unwrap()
        );
        assert!(
            std::fs::metadata(&frozen.control.path)
                .unwrap()
                .permissions()
                .readonly()
        );
        assert!(
            std::fs::metadata(&frozen.candidate.path)
                .unwrap()
                .permissions()
                .readonly()
        );
        assert_eq!(
            frozen.control.provenance.sha256,
            Some(ferric_bench::sha256_file(&control).unwrap())
        );
        assert_eq!(
            frozen.candidate.provenance.sha256,
            Some(ferric_bench::sha256_file(&candidate).unwrap())
        );
        let control_episode = EpisodeCoordinate {
            coordinate: AutonomyEvaluationCoordinate {
                arm: AutonomyArm::Control,
                harness_policy: HarnessPolicy::Legacy,
            },
            invocation: Invocation {
                ferric_bin: frozen.control.path.clone(),
                protocol: ActionProtocol::ConstrainedJson,
                openai: None,
                prompts_dir: None,
                keep_workspace: false,
            },
            child_policy_flag: None,
            frozen_binary: Some(frozen.control),
        };
        let candidate_episode = EpisodeCoordinate {
            coordinate: AutonomyEvaluationCoordinate {
                arm: AutonomyArm::Candidate,
                harness_policy: HarnessPolicy::Evidence,
            },
            invocation: Invocation {
                ferric_bin: frozen.candidate.path.clone(),
                protocol: ActionProtocol::ConstrainedJson,
                openai: None,
                prompts_dir: None,
                keep_workspace: false,
            },
            child_policy_flag: Some(HarnessPolicy::Evidence),
            frozen_binary: Some(frozen.candidate),
        };
        verify_episode_binary(&control_episode, "test control").unwrap();
        verify_episode_binary(&candidate_episode, "test candidate").unwrap();
    }

    #[test]
    fn single_evidence_freezes_and_records_the_verified_child_digest() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("selected-ferric.exe");
        std::fs::write(&source, b"evidence-a").unwrap();
        let source_sha256 = ferric_bench::sha256_file(&source).unwrap();

        let legacy_results = dir.path().join("legacy-results");
        let (legacy_path, legacy_frozen) = prepare_single_binary(
            &legacy_results,
            "legacy-run",
            HarnessPolicy::Legacy,
            source.clone(),
        )
        .unwrap();
        assert_eq!(legacy_path, source);
        assert!(legacy_frozen.is_none());
        assert!(!legacy_results.exists());

        let results = dir.path().join("evidence-results");
        let (invocation_path, frozen) = prepare_single_binary(
            &results,
            "evidence-run",
            HarnessPolicy::Evidence,
            source.clone(),
        )
        .unwrap();
        let frozen = frozen.expect("evidence policy must freeze its selected child");
        assert_eq!(invocation_path, frozen.path);
        assert_ne!(invocation_path, source);
        assert_eq!(
            ferric_bench::sha256_file(&invocation_path).unwrap(),
            source_sha256
        );
        assert_eq!(
            frozen.provenance.sha256.as_deref(),
            Some(source_sha256.as_str())
        );

        let cli = crate::Cli::try_parse_from([
            "ferric",
            "bench",
            "autonomy",
            "--harness-policy",
            "evidence",
        ])
        .unwrap();
        let crate::Command::Bench {
            command: crate::BenchCommand::Autonomy(args),
        } = cli.command
        else {
            panic!("expected autonomy command");
        };
        let invocation = Invocation {
            ferric_bin: invocation_path.clone(),
            protocol: ActionProtocol::ConstrainedJson,
            openai: None,
            prompts_dir: None,
            keep_workspace: false,
        };
        let recorded = provenance(&invocation, &args, "current", Some(&frozen.provenance));
        assert_eq!(
            recorded.binary.sha256.as_deref(),
            Some(source_sha256.as_str())
        );

        let expected_provenance = frozen.provenance.clone();
        drop(frozen);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::metadata(&invocation_path).unwrap().permissions();
            std::fs::set_permissions(
                &invocation_path,
                std::fs::Permissions::from_mode(permissions.mode() | 0o200),
            )
            .unwrap();
        }
        #[cfg(windows)]
        {
            let mut permissions = std::fs::metadata(&invocation_path).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            std::fs::set_permissions(&invocation_path, permissions).unwrap();
        }
        std::fs::write(&invocation_path, b"evidence-b").unwrap();
        let mutated = FrozenBinary {
            path: invocation_path.clone(),
            provenance: expected_provenance,
            _guard: std::fs::File::open(&invocation_path).unwrap(),
        };
        let episode = EpisodeCoordinate {
            coordinate: AutonomyEvaluationCoordinate {
                arm: AutonomyArm::Single,
                harness_policy: HarnessPolicy::Evidence,
            },
            invocation,
            child_policy_flag: Some(HarnessPolicy::Evidence),
            frozen_binary: Some(mutated),
        };
        let error = verify_episode_binary(&episode, "test mutation").unwrap_err();
        assert!(error.contains("SHA-256 changed"));
    }

    #[test]
    fn freezing_detects_a_source_change_during_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.exe");
        let destination = dir.path().join("frozen.exe");
        std::fs::write(&source, b"before").unwrap();
        let digest = ferric_bench::sha256_file(&source).unwrap();
        let error = copy_immutable_binary_with(&source, &destination, &digest, "control", || {
            std::fs::write(&source, b"after").unwrap()
        })
        .unwrap_err();
        assert!(error.contains("changed while it was frozen"));
    }

    #[test]
    fn managed_model_matching_requires_the_same_canonical_artifact() {
        let current = std::env::current_dir().unwrap();
        let file = tempfile::NamedTempFile::new_in(&current).unwrap();
        let absolute = file.path().to_string_lossy().to_string();
        let relative = file
            .path()
            .strip_prefix(&current)
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(managed_model_matches(&absolute, &relative));
        assert!(!managed_model_matches(
            "models/qwen/model.gguf",
            "artifacts/model.gguf"
        ));
        assert!(!managed_model_matches(
            "models/qwen/model.gguf",
            "models/qwen/other.gguf"
        ));

        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_model = first.path().join("same.gguf");
        let second_model = second.path().join("same.gguf");
        std::fs::write(&first_model, b"first").unwrap();
        std::fs::write(&second_model, b"second").unwrap();
        assert!(!managed_model_matches(
            &first_model.to_string_lossy(),
            &second_model.to_string_lossy()
        ));
    }

    #[test]
    fn paired_server_provenance_requires_known_single_slot_sampling_controls() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let model = model.display().to_string();
        let valid = ManagedServerProvenance {
            engine: "llama-server".to_string(),
            listener_base_url: "http://127.0.0.1:8080/v1".to_string(),
            model: Some(model.clone()),
            model_launch_argument: None,
            model_sha256: None,
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            gpu_layers: None,
            pid: None,
            listener_owner_pid: None,
            listener_port: None,
            engine_executable: None,
            engine_executable_sha256: None,
            engine_version: None,
            engine_argv: None,
        };
        validate_strict_server_provenance(&valid, &model, 8192).unwrap();

        let mut unknown_seed = valid.clone();
        unknown_seed.sampling_seed = None;
        assert!(
            validate_strict_server_provenance(&unknown_seed, &model, 8192)
                .unwrap_err()
                .contains("sampling_seed")
        );
        let mut parallel = valid.clone();
        parallel.parallel_slots = Some(2);
        assert!(
            validate_strict_server_provenance(&parallel, &model, 8192)
                .unwrap_err()
                .contains("exactly one")
        );
        let mut random_seed = valid.clone();
        random_seed.sampling_seed = Some(-1);
        assert!(
            validate_strict_server_provenance(&random_seed, &model, 8192)
                .unwrap_err()
                .contains("non-negative")
        );
        let mut unsupported_engine = valid.clone();
        unsupported_engine.engine = "ollama".to_string();
        assert!(
            validate_strict_server_provenance(&unsupported_engine, &model, 8192)
                .unwrap_err()
                .contains("llama-server")
        );
        assert!(
            validate_strict_server_provenance(&valid, &model, 4096)
                .unwrap_err()
                .contains("requires --ctx")
        );
    }

    #[test]
    fn live_server_snapshot_binding_requires_exact_process_argv_and_owner() {
        let model_dir = tempfile::tempdir().unwrap();
        let model = model_dir.path().join("example.gguf");
        std::fs::write(&model, b"model").unwrap();
        let model = model.display().to_string();
        let runfile = crate::server::ServerRunfile {
            schema_version: 1,
            engine: crate::server::Engine::LlamaServer,
            pid: 1234,
            port: 8080,
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            tailscale: false,
            tailscale_serve: None,
            model: Some(model.clone()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: None,
            origin_local_runfile: None,
        };
        let snapshot = crate::server::RegisteredServerSnapshot {
            pid: 1234,
            executable: PathBuf::from("bin/llama-server.exe"),
            argv: vec![
                "llama-server.exe".to_string(),
                "-m".to_string(),
                model.clone(),
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
            ],
            listener_owner_pid: 1234,
        };
        validate_live_server_snapshot(&runfile, &snapshot, &model, 8192).unwrap();

        let mut wrong_owner = snapshot.clone();
        wrong_owner.listener_owner_pid = 9999;
        assert!(
            validate_live_server_snapshot(&runfile, &wrong_owner, &model, 8192)
                .unwrap_err()
                .contains("PID")
        );
        let mut duplicated_seed = snapshot.clone();
        duplicated_seed
            .argv
            .extend(["--seed".to_string(), "42".to_string()]);
        assert!(
            validate_live_server_snapshot(&runfile, &duplicated_seed, &model, 8192)
                .unwrap_err()
                .contains("exactly once")
        );
        let mut wrong_model = snapshot.clone();
        wrong_model.argv[2] = "models/other.gguf".to_string();
        assert!(
            validate_live_server_snapshot(&runfile, &wrong_model, &model, 8192)
                .unwrap_err()
                .contains("model argv")
        );

        let mut split_endpoint = runfile.clone();
        split_endpoint.base_url = "http://127.0.0.1:8081/v1".to_string();
        assert!(
            validate_live_server_snapshot(&split_endpoint, &snapshot, &model, 8192)
                .unwrap_err()
                .contains("listener endpoint")
        );

        let other_dir = tempfile::tempdir().unwrap();
        let other_model = other_dir.path().join("example.gguf");
        std::fs::write(&other_model, b"other").unwrap();
        assert!(
            validate_live_server_snapshot(
                &runfile,
                &snapshot,
                &other_model.to_string_lossy(),
                8192,
            )
            .unwrap_err()
            .contains("not the managed artifact")
        );
    }

    #[test]
    fn paired_model_hash_is_computed_from_the_managed_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let expected = ferric_bench::sha256_file(&model).unwrap();
        assert_eq!(
            verify_managed_model_sha256(&model.to_string_lossy(), &expected).unwrap(),
            expected
        );
        assert!(
            verify_managed_model_sha256(&model.to_string_lossy(), &"0".repeat(64))
                .unwrap_err()
                .contains("mismatch")
        );
    }

    #[test]
    fn initial_tree_digest_is_portable_ordered_and_content_sensitive() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(left.path().join("src")).unwrap();
        std::fs::create_dir_all(right.path().join("src")).unwrap();
        std::fs::write(left.path().join("z.txt"), b"z").unwrap();
        std::fs::write(left.path().join("src/a.txt"), b"a").unwrap();
        std::fs::write(right.path().join("src/a.txt"), b"a").unwrap();
        std::fs::write(right.path().join("z.txt"), b"z").unwrap();
        assert_eq!(
            materialized_tree_sha256(left.path()).unwrap(),
            materialized_tree_sha256(right.path()).unwrap()
        );
        std::fs::write(right.path().join("z.txt"), b"changed").unwrap();
        assert_ne!(
            materialized_tree_sha256(left.path()).unwrap(),
            materialized_tree_sha256(right.path()).unwrap()
        );
    }

    #[test]
    fn retained_trace_names_cannot_collide_across_policy_arms() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("trace.jsonl");
        std::fs::write(&source, b"trace").unwrap();
        let results = dir.path().join("results");
        let control = retain_autonomy_trace(
            &source,
            &results,
            "run",
            &RetainedTraceKey {
                trial: 1,
                task_id: "H01",
                variant: "recovery",
                coordinate: AutonomyEvaluationCoordinate {
                    arm: AutonomyArm::Control,
                    harness_policy: HarnessPolicy::Legacy,
                },
                segment: 1,
            },
        )
        .unwrap();
        let candidate = retain_autonomy_trace(
            &source,
            &results,
            "run",
            &RetainedTraceKey {
                trial: 1,
                task_id: "H01",
                variant: "recovery",
                coordinate: AutonomyEvaluationCoordinate {
                    arm: AutonomyArm::Candidate,
                    harness_policy: HarnessPolicy::Evidence,
                },
                segment: 1,
            },
        )
        .unwrap();
        assert_ne!(control.relative_path, candidate.relative_path);
        assert!(control.relative_path.contains("control-legacy"));
        assert!(candidate.relative_path.contains("candidate-evidence"));
        assert_eq!(control.sha256, ferric_bench::sha256_bytes(b"trace"));
        assert_eq!(control.bytes, b"trace");
    }

    #[test]
    fn retained_trace_digest_and_validation_use_one_immutable_byte_snapshot() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source.jsonl");
        let canonical = std::fs::canonicalize(workspace.path()).unwrap();
        let mut sink = ferric_trace::JsonlSink::create_new(&source, "session").unwrap();
        sink.write_event(Event::SessionStart {
            workspace: canonical.display().to_string(),
            resumed_from: None,
        })
        .unwrap();
        sink.write_event(Event::PolicySelected {
            tier: ferric_core::Tier::Small,
            protocol: ActionProtocol::ConstrainedJson,
            harness_policy: HarnessPolicy::Evidence,
            max_turns: 7,
            max_tools: 8,
            prompt_budget_tokens: 4096,
            max_output_tokens: 1024,
            truncation_limit: 4000,
            tier_source: "params".to_string(),
        })
        .unwrap();
        drop(sink);
        let original = std::fs::read(&source).unwrap();
        let retained = retain_autonomy_trace(
            &source,
            &workspace.path().join("results"),
            "run",
            &RetainedTraceKey {
                trial: 1,
                task_id: "H01",
                variant: "current",
                coordinate: AutonomyEvaluationCoordinate {
                    arm: AutonomyArm::Single,
                    harness_policy: HarnessPolicy::Evidence,
                },
                segment: 1,
            },
        )
        .unwrap();
        std::fs::write(&source, b"tampered after retention").unwrap();

        assert_eq!(retained.bytes, original);
        assert_eq!(retained.sha256, ferric_bench::sha256_bytes(&original));
        assert_eq!(std::fs::read(&retained.absolute_path).unwrap(), original);
        analyze_trace_bytes(
            &retained.bytes,
            workspace.path(),
            ActionProtocol::ConstrainedJson,
            7,
            HarnessPolicy::Evidence,
        )
        .unwrap();
    }

    #[test]
    fn trace_analysis_accepts_empty_pre_response_timeout_prefix_without_counting_it() {
        let workspace = tempfile::tempdir().unwrap();
        let trace = workspace.path().join("pre-response-timeout.jsonl");
        let mut events = evidence_analysis_prefix(workspace.path());
        events.extend([
            Event::TurnStart { turn: 0 },
            Event::PromptAssembled {
                turn: 0,
                message_count: 2,
                chars: 10,
                offered_tools: vec!["read_file".to_string()],
            },
            Event::ConstraintApplied {
                kind: "json_schema".to_string(),
            },
        ]);
        write_analysis_trace(&trace, events);

        let analysis = analyze_trace(
            &trace,
            workspace.path(),
            ActionProtocol::ConstrainedJson,
            7,
            HarnessPolicy::Evidence,
        )
        .unwrap();
        assert_eq!(analysis.metrics.turns, 0);
        assert!(analysis.terminal.is_none());
    }

    #[test]
    fn trace_analysis_preserves_modern_pre_dispatch_retry_prefix() {
        let workspace = tempfile::tempdir().unwrap();
        let trace = workspace.path().join("pre-dispatch-timeout.jsonl");
        let call = ferric_core::ToolCall {
            id: "read-1".to_string(),
            name: "read_file".to_string(),
            args: serde_json::json!({"path": "a.txt"}),
        };
        let mut events = evidence_analysis_prefix(workspace.path());
        events.extend([
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(5),
                output_tokens: Some(1),
                truncated: false,
            },
            Event::ActionsProposed {
                turn: 0,
                calls: vec![call],
            },
        ]);
        write_analysis_trace(&trace, events);

        let analysis = analyze_trace(
            &trace,
            workspace.path(),
            ActionProtocol::ConstrainedJson,
            7,
            HarnessPolicy::Evidence,
        )
        .unwrap();
        assert_eq!(analysis.metrics.turns, 1);
        assert!(analysis.terminal.is_none());
    }

    #[test]
    fn typed_controller_mechanism_metrics_preserve_reason_breakdown() {
        let mut metrics = AutonomyTraceMetrics::default();
        for reason in [
            ControllerBlockReason::BlindMutation,
            ControllerBlockReason::SameTurnObservation,
            ControllerBlockReason::StaleObservation,
            ControllerBlockReason::UnsupportedMutation,
            ControllerBlockReason::RepairInspectionRequired,
            ControllerBlockReason::NoEffect,
            ControllerBlockReason::SyntaxRegression,
            ControllerBlockReason::RepeatedCheck,
        ] {
            record_controller_block(&mut metrics, reason);
        }
        assert_eq!(metrics.controller_blocks, 8);
        assert_eq!(metrics.blind_mutation_blocks, 1);
        assert_eq!(metrics.same_turn_observation_blocks, 1);
        assert_eq!(metrics.stale_observation_blocks, 1);
        assert_eq!(metrics.unsupported_mutation_blocks, 1);
        assert_eq!(metrics.repair_inspection_blocks, 1);
        assert_eq!(metrics.no_effect_blocks, 1);
        assert_eq!(metrics.syntax_regression_blocks, 1);
        assert_eq!(metrics.repeated_check_blocks, 1);

        let file = ObservationDetailV1::File(ferric_trace::FileObservationV1 {
            path: "src/lib.rs".to_string(),
            sha256: "file".to_string(),
            total_bytes: 1,
            total_lines: 1,
            requested_range: None,
            returned_range: None,
            complete: true,
            model_truncated: false,
        });
        let navigation = ferric_trace::NavigationObservationV1 {
            root: ".".to_string(),
            literal: "needle".to_string(),
            match_count: 1,
            max_results: 10,
            exhausted: true,
            result_sha256: "results".to_string(),
        };
        record_observation_metrics(&mut metrics, &file);
        record_observation_metrics(
            &mut metrics,
            &ObservationDetailV1::Search(navigation.clone()),
        );
        record_observation_metrics(&mut metrics, &ObservationDetailV1::Find(navigation));
        assert_eq!(metrics.observations_recorded, 3);
        assert_eq!(metrics.file_observations, 1);
        assert_eq!(metrics.search_observations, 1);
        assert_eq!(metrics.find_observations, 1);

        let mut diagnostics = BTreeSet::new();
        record_verification_check(
            &mut metrics,
            &mut diagnostics,
            &VerificationCheckV1 {
                version: 1,
                name: "test".to_string(),
                mutation_epoch: 1,
                attempt: 1,
                outcome: VerificationOutcome::Failed,
                diagnostic_sha256: Some("diagnostic".to_string()),
            },
        );
        record_verification_check(
            &mut metrics,
            &mut diagnostics,
            &VerificationCheckV1 {
                version: 1,
                name: "test".to_string(),
                mutation_epoch: 1,
                attempt: 2,
                outcome: VerificationOutcome::Failed,
                diagnostic_sha256: Some("diagnostic".to_string()),
            },
        );
        assert_eq!(metrics.verification_checks_failed, 2);
        assert_eq!(metrics.verification_repair_attempts, 1);
        assert_eq!(diagnostics.len(), 1);

        let first = TraceAnalysis {
            session: "one".to_string(),
            resumed_from: None,
            terminal: None,
            questions: Vec::new(),
            resume_prompts: Vec::new(),
            mutation_before_question: false,
            metrics,
            tool_turns: Vec::new(),
            failed_diagnostic_fingerprints: diagnostics,
        };
        let mut second_metrics = AutonomyTraceMetrics::default();
        record_controller_block(&mut second_metrics, ControllerBlockReason::BlindMutation);
        let second = TraceAnalysis {
            session: "two".to_string(),
            resumed_from: Some("one".to_string()),
            terminal: None,
            questions: Vec::new(),
            resume_prompts: Vec::new(),
            mutation_before_question: false,
            metrics: second_metrics,
            tool_turns: Vec::new(),
            failed_diagnostic_fingerprints: BTreeSet::from(["other".to_string()]),
        };
        let aggregate = aggregate_metrics(&[first, second]);
        assert_eq!(aggregate.controller_blocks, 9);
        assert_eq!(aggregate.blind_mutation_blocks, 2);
        assert_eq!(aggregate.distinct_failed_diagnostic_fingerprints, 2);
    }

    #[test]
    fn terminal_contract_requires_exact_task_complete() {
        assert!(terminal_matches(
            TerminalOutcome::Completed,
            Some("task_complete"),
            Some(0),
            false,
        ));
        assert!(!terminal_matches(
            TerminalOutcome::Completed,
            Some("final_text"),
            Some(0),
            false,
        ));
        assert!(terminal_matches(
            TerminalOutcome::Paused,
            Some("max_turns"),
            Some(1),
            false,
        ));
    }

    #[test]
    fn named_check_toml_preserves_multiline_python_without_shell() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checks.toml");
        write_named_checks(
            &path,
            &[AutonomyCheck {
                name: "verify".to_string(),
                argv: vec![
                    "{python}".to_string(),
                    "-c".to_string(),
                    "x = 1\nassert x == 1".to_string(),
                ],
                expected_exit: 0,
                stdout_regex: None,
                stderr_regex: None,
                timeout_s: 10,
            }],
            Path::new("python"),
        )
        .unwrap();
        let value: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["check"][0]["program"].as_str(), Some("python"));
        assert_eq!(
            value["check"][0]["args"][1].as_str(),
            Some("x = 1\nassert x == 1")
        );
    }

    #[test]
    fn tail_helper_is_utf8_safe_and_bounded() {
        let value = format!("{}END", "🦀".repeat(400));
        let tail = trim_tail(&value);
        assert!(tail.len() <= STDERR_TAIL_BYTES);
        assert!(tail.ends_with("END"));
    }

    #[test]
    fn model_preflight_reads_openai_and_ollama_shapes() {
        let value = serde_json::json!({
            "data": [{"id": "qwen.gguf"}],
            "models": [
                {"model": "qwen.gguf", "name": "ignored-duplicate"},
                {"name": "other.gguf"}
            ]
        });
        assert_eq!(
            model_ids(&value),
            vec!["other.gguf".to_string(), "qwen.gguf".to_string()]
        );
    }

    #[test]
    fn sha256_parser_is_exact_and_normalizes_case() {
        assert_eq!(parse_sha256(&"A".repeat(64)).unwrap(), "a".repeat(64));
        assert!(parse_sha256(&"a".repeat(63)).is_err());
        assert!(parse_sha256(&format!("{}z", "a".repeat(63))).is_err());
    }

    #[test]
    fn trace_analysis_binds_workspace_protocol_and_turn_budget() {
        let workspace = tempfile::tempdir().unwrap();
        let trace = workspace.path().join("metadata.jsonl");
        let canonical = std::fs::canonicalize(workspace.path()).unwrap();
        let mut sink = ferric_trace::JsonlSink::create_new(&trace, "session").unwrap();
        sink.write_event(Event::SessionStart {
            workspace: canonical.display().to_string(),
            resumed_from: None,
        })
        .unwrap();
        sink.write_event(Event::PolicySelected {
            tier: ferric_core::Tier::Small,
            protocol: ActionProtocol::ConstrainedJson,
            harness_policy: ferric_core::HarnessPolicy::Legacy,
            max_turns: 7,
            max_tools: 8,
            prompt_budget_tokens: 4096,
            max_output_tokens: 1024,
            truncation_limit: 4000,
            tier_source: "params".to_string(),
        })
        .unwrap();
        drop(sink);

        analyze_trace(
            &trace,
            workspace.path(),
            ActionProtocol::ConstrainedJson,
            7,
            HarnessPolicy::Legacy,
        )
        .unwrap();
        assert!(
            analyze_trace(
                &trace,
                workspace.path(),
                ActionProtocol::ConstrainedJson,
                8,
                HarnessPolicy::Legacy,
            )
            .unwrap_err()
            .contains("policy differs")
        );

        let other_workspace = tempfile::tempdir().unwrap();
        assert!(
            analyze_trace(
                &trace,
                other_workspace.path(),
                ActionProtocol::ConstrainedJson,
                7,
                HarnessPolicy::Legacy,
            )
            .unwrap_err()
            .contains("differs from episode workspace")
        );
    }

    #[test]
    fn recovery_clarification_rejects_an_unrelated_question() {
        let terms = vec!["timezone".to_string(), "UTC".to_string()];
        assert!(question_matches_terms(
            &["Which timezone should log timestamps use?"],
            &terms
        ));
        assert!(!question_matches_terms(
            &["Which file should I edit?"],
            &terms
        ));
    }
}
