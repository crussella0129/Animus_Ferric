//! Operational runner for Ferric's versioned internal autonomy matrix.
//!
//! Every episode uses the real `ferric query` process boundary. Recovery and
//! repository-brief variants chain the trace produced by one process into the
//! next; there is no offline/demo execution path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use ferric_bench::{
    AUTONOMY_RESULTS_SCHEMA_VERSION, AutonomyCategory, AutonomyCheck, AutonomyResultRow,
    AutonomyRunIssue, AutonomySegmentResult, AutonomyTask, AutonomyTraceMetrics, BinaryProvenance,
    EMBEDDED_AUTONOMY_V1, Invocation, ModelProvenance, OpenAiArgs, QuerySegmentRequest,
    RecoveryInjectionKind, ResumeProbeResult, ResumeRefusalMode, RunProvenance, TerminalOutcome,
    append_autonomy_row, autonomy_bench_spec, embedded_autonomy_suite, generate_repository_brief,
    preflight_command_checks, run_query_segment, summarize_autonomy_run,
    verify_command_checks_with_deadline, write_autonomy_summary,
};
use ferric_core::ActionProtocol;
use ferric_loop::TraceStructure;
use ferric_trace::{Event, ParsedEvent, TRACE_SCHEMA_VERSION, TraceReader};

use crate::query::ProtocolArg;

const PROVIDER_FAILURE_ENDPOINT: &str = "http://127.0.0.1:0/v1";
const STDERR_TAIL_BYTES: usize = 1_000;

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

    /// Preserve each materialized repository after grading.
    #[arg(long)]
    pub keep_workspace: bool,

    /// Validate and print the frozen corpus without running model episodes.
    #[arg(long)]
    pub list: bool,
}

pub fn run_autonomy(args: AutonomyArgs) -> ExitCode {
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
    if let Err(error) = preflight_autonomy_checks(&tasks, &args.python_bin) {
        eprintln!("autonomy check infrastructure: {error}");
        return ExitCode::FAILURE;
    }
    let resolved_api_base = crate::backend::resolved_base_url(args.api_base.as_deref());
    if let Err(error) = preflight_openai_server(
        &resolved_api_base,
        args.model.as_deref().expect("validated model"),
    ) {
        eprintln!("autonomy server preflight: {error}");
        return ExitCode::FAILURE;
    }

    let protocol: ActionProtocol = args.protocol.into();
    let ferric_bin = args
        .ferric_bin
        .clone()
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("ferric"));
    let openai = Some(OpenAiArgs {
        api_base: Some(resolved_api_base),
        model: args.model.clone().expect("validated model"),
        params_b: args.params_b,
        ctx: args.ctx,
    });
    let invocation = Invocation {
        ferric_bin,
        protocol,
        openai,
        prompts_dir: None,
        keep_workspace: args.keep_workspace,
    };

    let started_at = now_unix_ms();
    let run_id = new_run_id(started_at);
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
                println!("{} {} trial-{trial:03} — running", task.id, variant.label());
                match run_episode(
                    &suite.suite_id,
                    suite.schema_version,
                    &suite_sha256,
                    task,
                    *variant,
                    trial,
                    &run_id,
                    &invocation,
                    &args,
                ) {
                    Ok(row) => {
                        println!(
                            "{} {} trial-{trial:03} — contract {} / objective {} ({} ms)",
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
                                trial: Some(trial),
                                message: error.clone(),
                            });
                        }
                        if let Err(error) = append_autonomy_row(&args.results_dir, &row) {
                            eprintln!("cannot append autonomy row: {error}");
                            issues.push(AutonomyRunIssue {
                                task_id: Some(task.id.clone()),
                                variant: Some(variant.label().to_string()),
                                trial: Some(trial),
                                message: format!("cannot append result row: {error}"),
                            });
                        }
                        rows.push(row);
                    }
                    Err(error) => {
                        eprintln!("{} {} trial-{trial:03} — {error}", task.id, variant.label());
                        issues.push(AutonomyRunIssue {
                            task_id: Some(task.id.clone()),
                            variant: Some(variant.label().to_string()),
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
        .map(|task| (task.id.clone(), category_label(task.category).to_string()))
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

#[allow(clippy::too_many_arguments)]
fn run_episode(
    suite_id: &str,
    suite_schema_version: u32,
    suite_sha256: &str,
    task: &AutonomyTask,
    variant: AutonomyVariantArg,
    trial: u32,
    run_id: &str,
    invocation: &Invocation,
    args: &AutonomyArgs,
) -> Result<AutonomyResultRow, String> {
    let started_at = now_unix_ms();
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(task.timeout_s))
        .ok_or_else(|| format!("{} timeout is too large", task.id))?;
    let workspace = tempfile::tempdir().map_err(|error| format!("create workspace: {error}"))?;
    let profile_dir =
        tempfile::tempdir().map_err(|error| format!("create profile dir: {error}"))?;
    materialize_task(workspace.path(), task)?;
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
        };
        let process = run_query_segment(invocation, &request)
            .map_err(|error| format!("spawn query segment {segment}: {error}"))?;
        if let Some(error) = process.trace_discovery_error.clone() {
            infrastructure.push(error);
        }
        let mut observed = None;
        let mut retained = None;
        let mut retained_sha256 = None;
        let mut analysis = None;
        if let Some(trace) = process.trace_path.as_deref() {
            match analyze_trace(trace, workspace.path(), invocation.protocol, max_turns) {
                Ok(parsed) => {
                    observed = parsed.terminal.clone();
                    analysis = Some(parsed);
                }
                Err(error) => infrastructure.push(format!("segment {segment} trace: {error}")),
            }
            match retain_autonomy_trace(
                trace,
                &args.results_dir,
                run_id,
                trial,
                &task.id,
                variant.label(),
                segment,
            ) {
                Ok((path, digest)) => {
                    retained = Some(path);
                    retained_sha256 = Some(digest);
                }
                Err(error) => {
                    infrastructure.push(format!("retain segment {segment} trace: {error}"))
                }
            }
        } else if !process.timed_out {
            infrastructure.push(format!("segment {segment} produced no trace"));
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
        let Some(trace) = process.trace_path else {
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
                invocation,
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
            invocation,
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
    let requires_completed_state = expected_final == Some(TerminalOutcome::Completed);
    let check_spec = autonomy_bench_spec(task, suite_schema_version);
    let command_checks = if requires_completed_state {
        verify_command_checks_with_deadline(
            workspace.path(),
            &check_spec,
            &args.python_bin,
            remaining_before(deadline, task, "final grading")?,
        )
    } else {
        Vec::new()
    };
    if Instant::now() > deadline {
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
    let objective_completed = final_process_clean
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
    let contract_passed = sequence_matches
        && probes_passed
        && clarification_contract
        && (!requires_completed_state || objective_completed)
        && infrastructure.is_empty();

    let finished_at = now_unix_ms();
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
        provenance: provenance(invocation, args, variant.label()),
    };

    if args.keep_workspace {
        let kept = workspace.keep();
        println!(
            "{} {} trial-{trial:03} workspace: {}",
            task.id,
            variant.label(),
            kept.display()
        );
    }
    Ok(row)
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
}

fn analyze_trace(
    path: &Path,
    expected_workspace: &Path,
    expected_protocol: ActionProtocol,
    expected_max_turns: u32,
) -> Result<TraceAnalysis, String> {
    let reader = TraceReader::open(path).map_err(|error| error.to_string())?;
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
    let mut expected_session: Option<String> = None;
    let mut expected_seq = 0_u64;
    let mut saw_session_start = false;
    let mut saw_policy_selected = false;
    for record in reader {
        let record = record.map_err(|error| error.to_string())?;
        if record.v != TRACE_SCHEMA_VERSION {
            return Err(format!("unsupported trace schema {}", record.v));
        }
        if let Some(session) = &expected_session {
            if session != &record.session {
                return Err("trace mixes session identifiers".to_string());
            }
        } else {
            expected_session = Some(record.session.clone());
        }
        if record.seq != expected_seq {
            return Err(format!(
                "trace sequence gap: expected {expected_seq}, found {}",
                record.seq
            ));
        }
        expected_seq = expected_seq.saturating_add(1);
        let ParsedEvent::Known(event) = record.event else {
            return Err(format!("unknown trace event at sequence {}", record.seq));
        };
        structure.observe(&event)?;
        match &event {
            Event::SessionStart {
                workspace,
                resumed_from: source,
                ..
            } => {
                if saw_session_start || record.seq != 0 {
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
                max_turns,
                ..
            } => {
                if !saw_session_start || saw_policy_selected || record.seq != 1 {
                    return Err(
                        "trace must contain exactly one policy_selected after session_start"
                            .to_string(),
                    );
                }
                if *protocol != expected_protocol || *max_turns != expected_max_turns {
                    return Err(format!(
                        "trace policy differs from episode request: protocol {:?}/{:?}, max_turns {}/{}",
                        protocol, expected_protocol, max_turns, expected_max_turns
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
    if !saw_session_start || !saw_policy_selected {
        return Err("trace is missing session_start or policy_selected".to_string());
    }
    Ok(TraceAnalysis {
        session: expected_session.ok_or_else(|| "trace contains no events".to_string())?,
        resumed_from,
        terminal,
        questions,
        resume_prompts,
        mutation_before_question,
        metrics,
        tool_turns,
    })
}

fn aggregate_metrics(analyses: &[TraceAnalysis]) -> AutonomyTraceMetrics {
    let mut aggregate = AutonomyTraceMetrics::default();
    let mut tool_turns = Vec::new();
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
        aggregate.tools_called.extend(metrics.tools_called.clone());
        tool_turns.extend(&analysis.tool_turns);
    }
    if !tool_turns.is_empty() {
        let threshold = (tool_turns.len() * 4).div_ceil(5);
        aggregate.horizon_80_turn = tool_turns.get(threshold.saturating_sub(1)).copied();
    }
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
    trial: u32,
    task_id: &str,
    variant: &str,
    segment: u32,
) -> std::io::Result<(String, String)> {
    let relative =
        format!("traces/{run_id}/trial-{trial:03}-{task_id}-{variant}-s{segment:02}.jsonl");
    let destination = results_dir.join(&relative);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    let digest = ferric_bench::sha256_file(&results_dir.join(&relative))?;
    Ok((relative, digest))
}

fn run_workspace_mismatch_probe(
    invocation: &Invocation,
    trace: &Path,
    profile_dir: &Path,
    checks_file: &Path,
    task: &AutonomyTask,
    timeout: Duration,
) -> Result<ResumeProbeResult, String> {
    let other = tempfile::tempdir().map_err(|error| format!("create mismatch probe: {error}"))?;
    run_refusal_probe(
        "workspace_mismatch",
        invocation,
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
    invocation: &Invocation,
    trace: &Path,
    workspace: &Path,
    profile_dir: &Path,
    checks_file: &Path,
    task: &AutonomyTask,
    timeout: Duration,
) -> Result<ResumeProbeResult, String> {
    run_refusal_probe(
        "completed_session",
        invocation,
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
    invocation: &Invocation,
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
    };
    let process = run_query_segment(invocation, &request)
        .map_err(|error| format!("{mode} refusal probe: {error}"))?;
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
    let binary_path = std::fs::canonicalize(&invocation.ferric_bin)
        .unwrap_or_else(|_| invocation.ferric_bin.clone());
    let metadata = std::fs::metadata(&binary_path).ok();
    RunProvenance {
        ferric_version: env!("CARGO_PKG_VERSION").to_string(),
        git_commit: option_env!("FERRIC_GIT_COMMIT")
            .or(option_env!("VERGEN_GIT_SHA"))
            .or(option_env!("GITHUB_SHA"))
            .map(str::to_string),
        binary: BinaryProvenance {
            path: binary_path.display().to_string(),
            size_bytes: metadata.as_ref().map(std::fs::Metadata::len),
            modified_at_unix_ms: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_unix_ms),
            sha256: ferric_bench::sha256_file(&binary_path).ok(),
        },
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

        analyze_trace(&trace, workspace.path(), ActionProtocol::ConstrainedJson, 7).unwrap();
        assert!(
            analyze_trace(&trace, workspace.path(), ActionProtocol::ConstrainedJson, 8,)
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
