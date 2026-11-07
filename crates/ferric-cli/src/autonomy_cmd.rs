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
use std::time::{Duration, Instant};

use clap::{Args, ValueEnum};
use ferric_bench::{
    AUTONOMY_RESULTS_SCHEMA_VERSION, AutonomyEvaluationProvenance, AutonomyResultRow,
    AutonomyRunIssue, AutonomySegmentResult, AutonomyTask, AutonomyTraceMetrics, BinaryProvenance,
    CommandCheck, EMBEDDED_AUTONOMY_V1, Invocation, ManagedServerProvenance, MechanismCounts,
    ModelProvenance, OpenAiArgs, PolicyVariant, QuerySegmentRequest, RecoveryInjectionKind,
    ResumeProbeResult, ResumeRefusalMode, RetainedTraceValidation, RunProvenance,
    STDERR_TAIL_BYTES, TerminalOutcome, append_autonomy_row, autonomy_bench_spec,
    embedded_autonomy_suite, generate_repository_brief, git_commit, materialize_setup_files,
    new_run_id, now_unix_ms, parse_sha256, preflight_command_checks, run_query_segment,
    summarize_autonomy_run, verify_command_checks_with_deadline, write_autonomy_summary,
};
use ferric_core::{ActionProtocol, HarnessPolicy};
use ferric_loop::{StopReason, TraceStructure};
use ferric_trace::{
    ControllerBlockReason, Event, ObservationDetailV1, TRACE_SCHEMA_VERSION, VerificationCheckV1,
    VerificationOutcome,
};

use crate::config::ProtocolArg;

const PROVIDER_FAILURE_ENDPOINT: &str = "http://127.0.0.1:0/v1";

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

/// The autonomy matrix runs only the legacy controller (the paired
/// Legacy-vs-Evidence runner was retired by INT-0018). The flag remains so an
/// explicit `--harness-policy legacy` still reaches the child.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum AutonomyHarnessPolicyArg {
    Legacy,
}

impl From<AutonomyHarnessPolicyArg> for HarnessPolicy {
    fn from(policy: AutonomyHarnessPolicyArg) -> Self {
        match policy {
            AutonomyHarnessPolicyArg::Legacy => HarnessPolicy::Legacy,
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

impl From<AutonomyVariantArg> for PolicyVariant {
    fn from(variant: AutonomyVariantArg) -> Self {
        match variant {
            AutonomyVariantArg::Current => PolicyVariant::Current,
            AutonomyVariantArg::Recovery => PolicyVariant::Recovery,
            AutonomyVariantArg::RepositoryBrief => PolicyVariant::RepositoryBrief,
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

    /// Preserve each materialized repository after grading.
    #[arg(long)]
    pub keep_workspace: bool,

    /// Validate and print the frozen corpus without running model episodes.
    #[arg(long)]
    pub list: bool,
}

/// The child `ferric` every query segment of the matrix runs.
struct EpisodeChild {
    invocation: Invocation,
    /// `None` keeps the historical child argv: no `--harness-policy` flag.
    child_policy_flag: Option<HarnessPolicy>,
}

struct RetainedTraceKey<'a> {
    trial: u32,
    task_id: &'a str,
    variant: &'a str,
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
    let endpoint = match crate::backend::resolved_endpoint(args.api_base.as_deref()) {
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
    let managed_server = managed_server_provenance(&endpoint);
    if let Err(error) = preflight_openai_server(
        &resolved_api_base,
        args.model.as_deref().expect("validated model"),
    ) {
        eprintln!("autonomy server preflight: {error}");
        return ExitCode::FAILURE;
    }

    let episode = EpisodeChild {
        invocation: Invocation {
            ferric_bin: args
                .ferric_bin
                .clone()
                .or_else(|| std::env::current_exe().ok())
                .unwrap_or_else(|| PathBuf::from("ferric")),
            budget: None,
            protocol: args.protocol.into(),
            openai: Some(OpenAiArgs {
                api_base: Some(resolved_api_base),
                model: args.model.clone().expect("validated model"),
                params_b: args.params_b,
                ctx: args.ctx,
            }),
            prompts_dir: None,
            keep_workspace: args.keep_workspace,
        },
        child_policy_flag: args.harness_policy.map(HarnessPolicy::from),
    };
    let started_at = now_unix_ms();
    let run_id = new_run_id("autonomy", started_at);
    let suite_sha256 = ferric_bench::sha256_bytes(EMBEDDED_AUTONOMY_V1.as_bytes());
    let mut rows = Vec::new();
    let mut issues = Vec::new();

    for trial in 1..=args.trials {
        let offset = (trial - 1) as usize % tasks.len();
        for position in 0..tasks.len() {
            let task = tasks[(position + offset) % tasks.len()];
            let variant_offset = ((trial - 1) as usize + position) % variants.len();
            for variant_position in 0..variants.len() {
                let variant = variants[(variant_position + variant_offset) % variants.len()];
                println!(
                    "{} {} single legacy trial-{trial:03} — running",
                    task.id,
                    variant.label()
                );
                match run_episode(
                    &suite.suite_id,
                    suite.schema_version,
                    &suite_sha256,
                    task,
                    variant,
                    trial,
                    &run_id,
                    &episode,
                    managed_server.as_ref(),
                    &args,
                ) {
                    Ok(mut row) => {
                        println!(
                            "{} {} single legacy trial-{trial:03} — contract {} / objective {} ({} ms)",
                            task.id,
                            variant.label(),
                            pass_label(row.contract_passed),
                            pass_label(row.objective_completed),
                            row.wall_ms
                        );
                        if let Some(error) = &row.infrastructure_error {
                            issues.push(AutonomyRunIssue {
                                task_id: Some(task.id.clone()),
                                variant: Some(variant.label().to_string()),
                                harness_policy: Some(HarnessPolicy::Legacy),
                                trial: Some(trial),
                                message: error.clone(),
                            });
                        }
                        append_row_or_issue(&args.results_dir, &mut row, &mut issues);
                        rows.push(row);
                    }
                    Err(error) => {
                        eprintln!(
                            "{} {} single legacy trial-{trial:03} — {error}",
                            task.id,
                            variant.label()
                        );
                        issues.push(AutonomyRunIssue {
                            task_id: Some(task.id.clone()),
                            variant: Some(variant.label().to_string()),
                            harness_policy: Some(HarnessPolicy::Legacy),
                            trial: Some(trial),
                            message: error,
                        });
                    }
                }
            }
        }
    }

    let finished_at = now_unix_ms();
    let expected_tasks = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
    let expected_task_categories = tasks
        .iter()
        .map(|task| (task.id.clone(), task.category.label().to_string()))
        .collect::<BTreeMap<_, _>>();
    let expected_variants = variants
        .iter()
        .map(|variant| variant.label().to_string())
        .collect::<Vec<_>>();
    let summary = summarize_autonomy_run(
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
    variant: PolicyVariant,
    trial: u32,
    run_id: &str,
    episode: &EpisodeChild,
    managed_server: Option<&ManagedServerProvenance>,
    args: &AutonomyArgs,
) -> Result<AutonomyResultRow, String> {
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
    materialize_setup_files(workspace.path(), &task.setup_files)
        .map_err(|error| error.to_string())?;
    let initial_tree_sha256 = materialized_tree_sha256(workspace.path())?;
    let checks_file = profile_dir.path().join("checks.toml");
    write_named_checks(&checks_file, &task.checks, &args.python_bin)?;

    let (prompt, repository_brief_sha256, repository_brief_bytes, repository_brief_truncated) =
        if variant == PolicyVariant::RepositoryBrief {
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
        let api_base_override = (injection.map(|injection| injection.kind)
            == Some(RecoveryInjectionKind::ProviderFailure))
        .then_some(PROVIDER_FAILURE_ENDPOINT);
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
        let process = run_query_segment(&episode.invocation, &request)
            .map_err(|error| format!("spawn query segment {segment}: {error}"))?;
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
                        HarnessPolicy::Legacy,
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
        if analysis.is_some() && observed.is_none() && !process.timed_out {
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
            expected_terminal: expected_terminal.label().to_string(),
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

        if variant != PolicyVariant::Current
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

    if variant != PolicyVariant::Current
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
    let expected_resumes = if variant == PolicyVariant::Current {
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
    let row_provenance = provenance(&episode.invocation, args, variant.label());
    let child_binary_sha256 = row_provenance.binary.sha256.clone();
    let row = AutonomyResultRow {
        schema_version: AUTONOMY_RESULTS_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        suite_id: suite_id.to_string(),
        suite_schema_version,
        suite_sha256: suite_sha256.to_string(),
        task_id: task.id.clone(),
        task_name: task.name.clone(),
        category: task.category.label().to_string(),
        variant: variant.label().to_string(),
        harness_policy: HarnessPolicy::Legacy,
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
            harness_policy: HarnessPolicy::Legacy,
            workspace_instance_sha256: Some(workspace_instance_sha256),
            initial_tree_sha256: Some(initial_tree_sha256),
            child_binary_sha256,
            corpus_sha256: Some(suite_sha256.to_string()),
            model_sha256: args.model_sha256.clone(),
            query_temperature: Some("0.0".to_string()),
            managed_server: managed_server.cloned(),
        },
    };

    if args.keep_workspace {
        let kept = workspace.keep();
        println!(
            "{} {} single legacy trial-{trial:03} workspace: {}",
            task.id,
            variant.label(),
            kept.display()
        );
    }
    Ok(row)
}

/// The managed registration the endpoint was discovered from, if any.
fn managed_server_provenance(
    endpoint: &crate::backend::EndpointSelection,
) -> Option<ManagedServerProvenance> {
    let (_, server) = endpoint.managed()?;
    let runfile = &server.runfile;
    Some(ManagedServerProvenance {
        engine: runfile.engine.program().to_string(),
        listener_base_url: runfile.base_url.clone(),
        model: runfile.model.clone(),
        context_size: runfile.context_size,
        sampling_seed: runfile.sampling_seed,
        parallel_slots: runfile.parallel_slots,
    })
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

fn select_variants(selected: &[AutonomyVariantArg]) -> Vec<PolicyVariant> {
    let mut variants: Vec<PolicyVariant> = if selected.is_empty() {
        vec![
            PolicyVariant::Current,
            PolicyVariant::Recovery,
            PolicyVariant::RepositoryBrief,
        ]
    } else {
        selected.iter().copied().map(PolicyVariant::from).collect()
    };
    variants.sort();
    variants.dedup();
    variants
}

fn expected_terminals(task: &AutonomyTask, variant: PolicyVariant) -> Vec<TerminalOutcome> {
    if variant == PolicyVariant::Current {
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

fn write_named_checks(path: &Path, checks: &[CommandCheck], python: &Path) -> Result<(), String> {
    let mut named = Vec::with_capacity(checks.len());
    for check in checks {
        if check.expected_exit != 0 || check.stdout_regex.is_some() || check.stderr_regex.is_some()
        {
            return Err(format!(
                "check `{}` cannot be used as an in-loop completion gate: only exit 0 without output regex is supported",
                check.name
            ));
        }
        let program = match check.argv.first().map(String::as_str) {
            Some("{python}") => python.to_path_buf(),
            program => PathBuf::from(program.unwrap_or_default()),
        };
        named.push(ferric_tools::NamedCheck {
            name: check.name.clone(),
            program,
            args: check.argv.iter().skip(1).cloned().collect(),
            timeout_s: check.timeout_s,
            output_limit: 16000,
        });
    }
    let text = toml::to_string(&crate::query::NamedChecksFile { checks: named })
        .map_err(|error| format!("encode {}: {error}", path.display()))?;
    std::fs::write(path, text).map_err(|error| format!("write {}: {error}", path.display()))
}

fn preflight_autonomy_checks(tasks: &[&AutonomyTask], python: &Path) -> Result<(), String> {
    let specs: Vec<_> = tasks
        .iter()
        .map(|task| autonomy_bench_spec(task, 1))
        .collect();
    preflight_command_checks(&specs, python)
}

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
                record_observation_metrics(&mut metrics.mechanisms, &observation.detail);
            }
            Event::ControllerBlocked { block, .. } => {
                record_controller_block(&mut metrics.mechanisms, block.reason);
            }
            Event::WorkspaceEffectRecorded { effect, .. } => {
                let mechanisms = &mut metrics.mechanisms;
                mechanisms.workspace_effects_recorded += 1;
                mechanisms.workspace_effect_paths = mechanisms
                    .workspace_effect_paths
                    .saturating_add(effect.effects.len() as u32);
            }
            Event::VerificationCheckRecorded { check, .. } => {
                record_verification_check(
                    &mut metrics.mechanisms,
                    &mut failed_diagnostic_fingerprints,
                    check,
                );
            }
            Event::ControllerCheckpoint { .. } => metrics.mechanisms.controller_checkpoints += 1,
            Event::RecoveryPacketInjected { .. } => {
                metrics.mechanisms.recovery_packets_injected += 1
            }
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
    metrics.mechanisms.distinct_failed_diagnostic_fingerprints =
        failed_diagnostic_fingerprints.len() as u32;
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

fn record_observation_metrics(metrics: &mut MechanismCounts, detail: &ObservationDetailV1) {
    metrics.observations_recorded += 1;
    match detail {
        ObservationDetailV1::File(_) => metrics.file_observations += 1,
        ObservationDetailV1::Search(_) => metrics.search_observations += 1,
        ObservationDetailV1::Find(_) => metrics.find_observations += 1,
    }
}

fn record_controller_block(metrics: &mut MechanismCounts, reason: ControllerBlockReason) {
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
    metrics: &mut MechanismCounts,
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
        aggregate.mechanisms.add(&metrics.mechanisms);
        failed_diagnostic_fingerprints
            .extend(analysis.failed_diagnostic_fingerprints.iter().cloned());
        aggregate.tools_called.extend(metrics.tools_called.clone());
        tool_turns.extend(&analysis.tool_turns);
    }
    if !tool_turns.is_empty() {
        let threshold = (tool_turns.len() * 4).div_ceil(5);
        aggregate.horizon_80_turn = tool_turns.get(threshold.saturating_sub(1)).copied();
    }
    // Distinct across the whole episode, not the sum of per-segment counts.
    aggregate.mechanisms.distinct_failed_diagnostic_fingerprints =
        failed_diagnostic_fingerprints.len() as u32;
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
        // Only refusal probes expect a rejected resume; it is never a
        // validated segment outcome.
        TerminalOutcome::ResumeRejected => false,
    }
}

fn injection_matches(kind: Option<RecoveryInjectionKind>, observed: Option<&str>) -> bool {
    kind.is_none_or(|kind| Some(kind.expected_stop_reason()) == observed)
}

/// A resumable stop: any operational failure the loop records, excluding a
/// clarification pause (`needs_input`).
fn is_pause_terminal(reason: &str) -> bool {
    StopReason::parse(reason).is_some_and(StopReason::is_failure)
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
        "traces/{run_id}/trial-{:03}-{}-{}-single-legacy-s{:02}.jsonl",
        key.trial, key.task_id, key.variant, key.segment,
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
    episode: &EpisodeChild,
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
    episode: &EpisodeChild,
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
    episode: &EpisodeChild,
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
    let process = run_query_segment(&episode.invocation, &request)
        .map_err(|error| format!("spawn {mode} refusal probe: {error}"))?;
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

fn provenance(invocation: &Invocation, args: &AutonomyArgs, variant: &str) -> RunProvenance {
    let binary = BinaryProvenance::inspect(&invocation.ferric_bin);
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
        git_commit: child_matches_runner.then(git_commit).flatten(),
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

static WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn evidence_analysis_prefix(workspace: &Path) -> Vec<Event> {
        let controller = ferric_loop::ControllerState::new(Vec::<String>::new()).unwrap();
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

    /// The paired runner's options and the Evidence policy are retired
    /// (INT-0018); omission and `--harness-policy legacy` keep their child argv.
    #[test]
    fn single_binary_parser_keeps_only_the_legacy_child_policy() {
        let parse = |extra: &[&str]| {
            let mut argv = vec!["ferric", "bench", "autonomy"];
            argv.extend_from_slice(extra);
            crate::Cli::try_parse_from(argv)
        };
        let child_policy = |extra: &[&str]| match parse(extra).unwrap().command {
            Some(crate::Command::Bench {
                command: crate::BenchCommand::Autonomy(args),
            }) => args.harness_policy.map(HarnessPolicy::from),
            _ => panic!("expected autonomy command"),
        };
        assert_eq!(child_policy(&[]), None);
        assert_eq!(
            child_policy(&["--harness-policy", "legacy"]),
            Some(HarnessPolicy::Legacy)
        );
        let digest = "b".repeat(64);
        for retired in [
            &["--harness-policy", "evidence"][..],
            &["--harness-policy", "evidence-planner"],
            &["--paired"],
            &["--control-bin", "control-ferric.exe"],
            &["--candidate-bin", "candidate-ferric.exe"],
            &["--control-sha256", digest.as_str()],
        ] {
            assert!(parse(retired).is_err(), "{retired:?}");
        }
    }

    #[test]
    fn external_child_does_not_inherit_runner_build_metadata() {
        let cli = crate::Cli::try_parse_from(["ferric", "bench", "autonomy"]).unwrap();
        let Some(crate::Command::Bench {
            command: crate::BenchCommand::Autonomy(args),
        }) = cli.command
        else {
            panic!("expected autonomy command");
        };
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("ferric-child.exe");
        std::fs::write(&child, b"external child bytes").unwrap();
        let invocation = Invocation {
            ferric_bin: child.clone(),
            budget: None,
            protocol: ActionProtocol::ConstrainedJson,
            openai: None,
            prompts_dir: None,
            keep_workspace: false,
        };
        let provenance = provenance(&invocation, &args, "current");
        assert_eq!(
            provenance.binary.sha256,
            ferric_bench::sha256_file(&child).ok()
        );
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
            vec![PolicyVariant::Current, PolicyVariant::RepositoryBrief]
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
    fn retained_trace_name_keeps_the_single_legacy_layout() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("trace.jsonl");
        std::fs::write(&source, b"trace").unwrap();
        let retained = retain_autonomy_trace(
            &source,
            &dir.path().join("results"),
            "run",
            &RetainedTraceKey {
                trial: 1,
                task_id: "H01",
                variant: "recovery",
                segment: 1,
            },
        )
        .unwrap();
        assert_eq!(
            retained.relative_path,
            "traces/run/trial-001-H01-recovery-single-legacy-s01.jsonl"
        );
        assert_eq!(retained.sha256, ferric_bench::sha256_bytes(b"trace"));
        assert_eq!(retained.bytes, b"trace");
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
            record_controller_block(&mut metrics.mechanisms, reason);
        }
        assert_eq!(metrics.mechanisms.controller_blocks, 8);
        assert_eq!(metrics.mechanisms.blind_mutation_blocks, 1);
        assert_eq!(metrics.mechanisms.same_turn_observation_blocks, 1);
        assert_eq!(metrics.mechanisms.stale_observation_blocks, 1);
        assert_eq!(metrics.mechanisms.unsupported_mutation_blocks, 1);
        assert_eq!(metrics.mechanisms.repair_inspection_blocks, 1);
        assert_eq!(metrics.mechanisms.no_effect_blocks, 1);
        assert_eq!(metrics.mechanisms.syntax_regression_blocks, 1);
        assert_eq!(metrics.mechanisms.repeated_check_blocks, 1);

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
        record_observation_metrics(&mut metrics.mechanisms, &file);
        record_observation_metrics(
            &mut metrics.mechanisms,
            &ObservationDetailV1::Search(navigation.clone()),
        );
        record_observation_metrics(
            &mut metrics.mechanisms,
            &ObservationDetailV1::Find(navigation),
        );
        assert_eq!(metrics.mechanisms.observations_recorded, 3);
        assert_eq!(metrics.mechanisms.file_observations, 1);
        assert_eq!(metrics.mechanisms.search_observations, 1);
        assert_eq!(metrics.mechanisms.find_observations, 1);

        let mut diagnostics = BTreeSet::new();
        record_verification_check(
            &mut metrics.mechanisms,
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
            &mut metrics.mechanisms,
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
        assert_eq!(metrics.mechanisms.verification_checks_failed, 2);
        assert_eq!(metrics.mechanisms.verification_repair_attempts, 1);
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
        record_controller_block(
            &mut second_metrics.mechanisms,
            ControllerBlockReason::BlindMutation,
        );
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
        assert_eq!(aggregate.mechanisms.controller_blocks, 9);
        assert_eq!(aggregate.mechanisms.blind_mutation_blocks, 2);
        assert_eq!(
            aggregate.mechanisms.distinct_failed_diagnostic_fingerprints,
            2
        );
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
            &[CommandCheck {
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
        let parsed: crate::query::NamedChecksFile =
            toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            parsed.checks,
            [ferric_tools::NamedCheck {
                name: "verify".to_string(),
                program: PathBuf::from("python"),
                args: vec!["-c".to_string(), "x = 1\nassert x == 1".to_string()],
                timeout_s: 10,
                output_limit: 16000,
            }]
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
