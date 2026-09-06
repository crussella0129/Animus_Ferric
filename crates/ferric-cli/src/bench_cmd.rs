//! `ferric bench` — run the L0–L6 ladder, append results.jsonl, calibrate.
//!
//! Spawns this same binary's `query` subcommand per level. External inference
//! speed belongs to the selected server/runtime, not this HTTP client's build
//! profile. `--mock` is the source-test path (no model needed).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Args;
use ferric_bench::{
    AttemptIdentity, BenchSpec, BinaryProvenance, BudgetControls, Invocation, ModelProfileRecord,
    ModelProvenance, OpenAiArgs, ResolvedAgentBudget, ResultRow, RunBudgetEvidence, RunIssue,
    RunProvenance, RunSummary, append_row, calibrate_from_evidence, completed, embedded_specs,
    failure_admission, parse_trace, preflight_command_checks, retain_budget_trace,
    run_spec_with_budget, summarize_run, verify_command_checks, verify_expectations, verify_tools,
    write_summary,
};
use ferric_core::{ActionProtocol, tier_for_params};

use crate::query::ProtocolArg;

#[derive(Args)]
pub struct BenchArgs {
    /// Levels to run (repeatable). Empty = all embedded levels (0..=6).
    #[arg(long)]
    pub level: Vec<u8>,

    /// Action protocol for the runs.
    #[arg(long, value_enum, default_value = "grammar")]
    pub protocol: ProtocolArg,

    /// Variant label recorded in each results row.
    #[arg(long, default_value = "default")]
    pub variant: String,

    /// OpenAI-compatible base URL (openai backend; omit to auto-discover a
    /// running `ferric server`).
    #[arg(long)]
    pub api_base: Option<String>,

    /// Model identifier for the openai backend (e.g. `qwen2.5-coder:7b`).
    #[arg(long)]
    pub model: Option<String>,

    /// SHA-256 of the model artifact when known. Remote model identifiers do
    /// not imply a file digest, so the default is intentionally unknown.
    #[arg(long, value_parser = parse_sha256)]
    pub model_sha256: Option<String>,

    /// Fleet sweep (openai backend): run the full L0–L6 ladder for each
    /// comma-separated model id and print a `measured_level` leaderboard. One
    /// profile record per model. Overrides `--model`.
    #[arg(long)]
    pub models: Option<String>,

    #[arg(long, default_value_t = 1.2)]
    pub params_b: f32,

    #[arg(long, default_value_t = 4096)]
    pub ctx: u32,

    /// Multiply only agent execution deadlines (positive finite; default 1).
    /// Grader, startup, capture and cleanup bounds are unchanged. A non-default
    /// scale is diagnostic: profiles are left unchanged; evidence is retained.
    #[arg(long, default_value_t = 1.0)]
    pub timeout_scale: f64,

    /// Explicit main-action output cap; constrained by declared context reserve.
    /// Does not retune reasoning or compaction. Any explicit cap is diagnostic:
    /// no calibrated profile is published, even when it equals a tier default.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_output_tokens: Option<u32>,

    /// Prompt-element library passed to each run.
    #[arg(long)]
    pub prompts_dir: Option<PathBuf>,

    /// Python executable used by authoritative L3-L6 checks.
    #[arg(long, default_value = "python")]
    pub python_bin: PathBuf,

    /// Number of complete sweeps to run (each trial rotates the level order).
    #[arg(
        long,
        default_value_t = 1,
        value_parser = clap::value_parser!(u32).range(1..=100)
    )]
    pub trials: u32,

    /// Per-level pass rate required for calibration qualification.
    #[arg(long, default_value_t = 0.90, value_parser = parse_pass_rate)]
    pub min_pass_rate: f64,

    /// Evidence destination: results.jsonl, summaries and trace budget sidecars.
    /// Eligible default full sweeps may also publish model_profiles.json.
    #[arg(long, default_value = "benchmarks")]
    pub results_dir: PathBuf,

    /// Keep each run's workspace instead of deleting it.
    #[arg(long)]
    pub keep_workspace: bool,

    /// Override the spawned binary (default: this executable).
    #[arg(long)]
    pub ferric_bin: Option<PathBuf>,

    /// Run against the built-in mock instead of a real model.
    #[arg(long)]
    pub mock: bool,
}

pub fn run_bench(args: BenchArgs) -> ExitCode {
    if let Err(error) =
        crate::config::validate_effective_numbers(args.params_b, args.ctx, 0.0, None)
    {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    if args.models.is_some() && args.model_sha256.is_some() {
        eprintln!("--model-sha256 is only valid with one --model, not --models");
        return ExitCode::FAILURE;
    }
    let all_specs = match embedded_specs() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bad embedded specs: {e}");
            return ExitCode::FAILURE;
        }
    };
    let selected: Vec<_> = all_specs
        .into_iter()
        .filter(|s| args.level.is_empty() || args.level.contains(&s.level))
        .collect();
    if selected.is_empty() {
        eprintln!("no matching levels for {:?}", args.level);
        return ExitCode::FAILURE;
    }
    // Resolve every selected deadline exactly once, before preflight or any
    // benchmark result/workspace effects. The same values serve every trial.
    let controls = match BudgetControls::new(
        args.timeout_scale,
        args.max_output_tokens,
        args.params_b,
        args.ctx,
    ) {
        Ok(controls) => controls,
        Err(error) => {
            eprintln!("invalid benchmark budget: {error}");
            return ExitCode::FAILURE;
        }
    };
    let budgets = match selected
        .iter()
        .map(|spec| controls.resolve_agent(spec.timeout_s))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(budgets) => budgets,
        Err(error) => {
            eprintln!("invalid benchmark budget: {error}");
            return ExitCode::FAILURE;
        }
    };
    // No effective override keeps historical argv, including frozen mock and
    // continuation controls. Parent deadline attribution is still retained.
    let child_budget =
        (args.timeout_scale != 1.0 || args.max_output_tokens.is_some()).then_some(controls);
    if let Err(e) = preflight_command_checks(&selected, &args.python_bin) {
        eprintln!("benchmark check infrastructure: {e}");
        return ExitCode::FAILURE;
    }

    let protocol: ActionProtocol = args.protocol.into();
    let ferric_bin = args
        .ferric_bin
        .clone()
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("ferric"));
    let full_ladder = args.level.is_empty();

    // Fleet sweep (openai): the full L0–L6 ladder per `--models` id + a
    // measured_level leaderboard. The fleet case is ollama model ids.
    if let Some(models_csv) = &args.models {
        let model_ids: Vec<String> = models_csv
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if model_ids.is_empty() {
            eprintln!("--models is empty");
            return ExitCode::FAILURE;
        }
        let mut board: Vec<ModelProfileRecord> = Vec::new();
        let mut infrastructure_ok = true;
        for model_id in &model_ids {
            println!("\n=== {model_id} ===");
            let inv = Invocation {
                budget: child_budget.clone(),
                ferric_bin: ferric_bin.clone(),
                protocol,
                openai: Some(OpenAiArgs {
                    api_base: args.api_base.clone(),
                    model: model_id.clone(),
                    params_b: args.params_b,
                    ctx: args.ctx,
                }),
                prompts_dir: args.prompts_dir.clone(),
                keep_workspace: args.keep_workspace,
            };
            let outcome = run_trials(
                &selected,
                &budgets,
                &inv,
                protocol,
                &Some(model_id.clone()),
                full_ladder,
                &args,
            );
            if !outcome.infrastructure_ok {
                eprintln!("{model_id}: benchmark infrastructure failed; profile left unchanged");
                infrastructure_ok = false;
                continue;
            }
            let record = match publish_calibration(
                &args.results_dir,
                model_id,
                args.params_b,
                &ferric_core::protocol_key(protocol),
                &outcome.summary.calibration,
            ) {
                Ok(ProfilePublication::Published(record)) => *record,
                Ok(ProfilePublication::Diagnostic) => continue,
                Ok(ProfilePublication::Ineligible(reason)) => {
                    println!("{model_id}: profile left unchanged ({reason})");
                    continue;
                }
                Err(error) => {
                    eprintln!("cannot write model profile: {error}");
                    infrastructure_ok = false;
                    continue;
                }
            };
            match record.measured_level {
                Some(level) => println!(
                    "calibrated {model_id}: measured_level {level} ({} -> {})",
                    record.tier_from_params,
                    record.tier_from_measured.as_deref().unwrap_or("?")
                ),
                None => println!("calibrated {model_id}: measured_level none (failed L0)"),
            }
            board.push(record);
        }
        // Leaderboard: highest measured_level first (ADR-008).
        board.sort_by(|a, b| {
            b.measured_level
                .cmp(&a.measured_level)
                .then_with(|| a.model.cmp(&b.model))
        });
        if child_budget
            .as_ref()
            .is_some_and(BudgetControls::is_diagnostic)
        {
            println!("\nDiagnostic sweep — no calibrated leaderboard was published.");
        } else {
            println!("\n# Agentic Capability Leaderboard (L0-L6)");
            println!("| Model | measured_level | tier |");
            println!("|-------|----------------|------|");
        }
        for r in &board {
            println!(
                "| {} | {} | {} |",
                r.model,
                r.measured_level
                    .map(|l| l.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                r.tier_from_measured.as_deref().unwrap_or("-")
            );
        }
        // A low measured_level is a valid measurement, not a failure.
        return if infrastructure_ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    // Single-model path. openai = ollama/llama-server; `model_name`
    // keys the calibration record (model-id for openai).
    let (openai, model_name) = if args.mock {
        (None, None)
    } else {
        let Some(model_id) = args.model.clone() else {
            eprintln!("--model <id> is required (or use --mock)");
            return ExitCode::FAILURE;
        };
        let oa = OpenAiArgs {
            api_base: args.api_base.clone(),
            model: model_id.clone(),
            params_b: args.params_b,
            ctx: args.ctx,
        };
        (Some(oa), Some(model_id))
    };
    let inv = Invocation {
        budget: child_budget,
        ferric_bin,
        protocol,
        openai,
        prompts_dir: args.prompts_dir.clone(),
        keep_workspace: args.keep_workspace,
    };

    let outcome = run_trials(
        &selected,
        &budgets,
        &inv,
        protocol,
        &model_name,
        full_ladder,
        &args,
    );
    let mut infrastructure_ok = outcome.infrastructure_ok;

    // Calibrate from this sweep's rows — but ONLY from a full ladder.
    //
    // A partial `--level` sweep is a diagnostic, not a calibration. Writing a
    // profile from one level silently DOWNGRADED a fuller result: investigating
    // qwen2.5-coder-7b's L5 rewrote its record from measured_level 6 (Large) to
    // 5 (Medium), and `ferric query` reads that profile to size its policy
    // (ADR-029/086). Looking at one rung must not change the model's tier.
    if model_name.is_some() && !full_ladder && !outcome.summary.calibration.is_diagnostic() {
        println!(
            "partial sweep ({} level(s)) — profile left unchanged; run without --level to recalibrate",
            selected.len()
        );
    }
    if let Some(model_name) = &model_name
        && full_ladder
        && infrastructure_ok
    {
        match publish_calibration(
            &args.results_dir,
            model_name,
            args.params_b,
            &ferric_core::protocol_key(protocol),
            &outcome.summary.calibration,
        ) {
            Ok(ProfilePublication::Diagnostic) => {}
            Ok(ProfilePublication::Ineligible(reason)) => {
                eprintln!("calibration evidence is not eligible: {reason}");
                return ExitCode::FAILURE;
            }
            Ok(ProfilePublication::Published(record)) => {
                if let Some(level) = record.measured_level {
                    println!(
                        "calibrated {model_name}: measured_level {level} ({} -> {})",
                        record.tier_from_params,
                        record.tier_from_measured.as_deref().unwrap_or("?")
                    );
                }
            }
            Err(error) => {
                eprintln!("cannot write model profile: {error}");
                infrastructure_ok = false;
            }
        }
    }
    if model_name.is_some() && full_ladder && !infrastructure_ok {
        eprintln!("benchmark infrastructure failed; profile left unchanged");
    }

    if outcome.qualified && infrastructure_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

struct BenchRunOutcome {
    qualified: bool,
    infrastructure_ok: bool,
    summary: RunSummary,
}

#[derive(Debug)]
enum ProfilePublication {
    Diagnostic,
    Ineligible(String),
    Published(Box<ModelProfileRecord>),
}

/// The actual shared single/fleet publication boundary. Diagnostic evidence
/// returns before even reading the profile store, including malformed bytes.
fn publish_calibration(
    results_dir: &Path,
    model: &str,
    params_b: f32,
    protocol: &str,
    evidence: &ferric_bench::CalibrationEvidence,
) -> std::io::Result<ProfilePublication> {
    if evidence.is_diagnostic() {
        return Ok(ProfilePublication::Diagnostic);
    }
    let Some(record) = calibrate_from_evidence(model, params_b, protocol, evidence) else {
        return Ok(ProfilePublication::Ineligible(
            evidence
                .ineligible_reason
                .clone()
                .unwrap_or_else(|| "calibration evidence was not eligible".into()),
        ));
    };
    ferric_bench::write_profile(results_dir, &record)?;
    Ok(ProfilePublication::Published(Box::new(record)))
}

fn trial_outcome_label(row: &ResultRow, row_written: bool) -> String {
    let observed = if row.timed_out {
        "PARENT TIMEOUT"
    } else {
        match row.terminator.as_deref() {
            Some("truncated_action") => "OUTPUT LIMIT (truncated_action)",
            Some("provider_error") => "PROVIDER ERROR (provider_error)",
            _ if row.completed => "PASS",
            _ => "FAIL",
        }
    };
    let label = if !row_written || row.infrastructure_error.is_some() {
        format!("INFRASTRUCTURE FAILURE; observed {observed}")
    } else {
        observed.to_string()
    };
    if row
        .budget
        .as_ref()
        .is_some_and(|budget| budget.controls.is_diagnostic())
    {
        format!("DIAGNOSTIC — {label}")
    } else {
        label
    }
}

/// Run every requested trial for one model, rotating the first level on each
/// trial so persistent ordering effects are attributable rather than fixed.
fn run_trials(
    selected: &[BenchSpec],
    budgets: &[ResolvedAgentBudget],
    inv: &Invocation,
    protocol: ActionProtocol,
    model_name: &Option<String>,
    full_ladder: bool,
    args: &BenchArgs,
) -> BenchRunOutcome {
    let started_at_unix_ms = now_unix_ms();
    let run_id = new_run_id(started_at_unix_ms);
    let mut rows: Vec<ResultRow> = Vec::new();
    let mut issues = Vec::new();

    for trial_index in 0..args.trials {
        let trial_id = format!("trial-{:03}", trial_index + 1);
        let trial_prefix = if args.trials > 1 {
            format!("{trial_id} ")
        } else {
            String::new()
        };
        if args.trials > 1 {
            println!("\n--- {trial_id} of {} ---", args.trials);
        }
        let offset = trial_index as usize % selected.len();
        for position in 0..selected.len() {
            let spec_index = (position + offset) % selected.len();
            let spec = &selected[spec_index];
            let attempt_started_at = now_unix_ms();
            let record = match run_spec_with_budget(spec, inv, &budgets[spec_index]) {
                Ok(record) => record,
                Err(error) => {
                    let message = format!("cannot execute benchmark child: {error}");
                    eprintln!("{trial_prefix}L{} run error: {message}", spec.level);
                    issues.push(RunIssue {
                        trial_id: Some(trial_id.clone()),
                        level: Some(spec.level),
                        message,
                    });
                    continue;
                }
            };

            let mut attempt_issues = Vec::new();
            let mut budget = record.budget.clone().unwrap_or_else(|| {
                attempt_issues.push("runner omitted required budget attribution".to_string());
                budgets[spec_index].evidence(record.exit_code, record.timed_out)
            });
            if let Err(error) = budget.observe_trace(record.trace_path.as_deref()) {
                attempt_issues.push(format!("cannot observe trace budget: {error}"));
            }
            if let ferric_bench::TraceEvidenceState::Malformed { error } = &budget.trace.state {
                attempt_issues.push(format!("malformed trace budget evidence: {error}"));
            }
            let retained_trace = match record.trace_path.as_deref() {
                Some(source) => {
                    let retained = AttemptIdentity::new(&run_id, &trial_id, spec.level)
                        .and_then(|identity| {
                            retain_budget_trace(source, &args.results_dir, identity, &budget)
                        })
                        .and_then(|reference| {
                            ferric_bench::verify_budget_trace(&args.results_dir, &reference)
                                .map(|sidecar| (reference, sidecar.evidence))
                        });
                    match retained {
                        Ok((reference, retained_budget)) => {
                            let path = reference.trace_path.clone();
                            budget = retained_budget;
                            if let ferric_bench::TraceEvidenceState::Malformed { error } =
                                &budget.trace.state
                            {
                                attempt_issues.push(format!(
                                    "malformed retained trace budget evidence: {error}"
                                ));
                            }
                            Some(path)
                        }
                        Err(error) => {
                            attempt_issues.push(format!("cannot retain trace: {error}"));
                            None
                        }
                    }
                }
                None => {
                    attempt_issues.push("benchmark child produced no trace".to_string());
                    None
                }
            };
            // Successful rows derive their metrics from the exact retained
            // evidence, not another read of the disposable child workspace.
            let metrics_path = retained_trace
                .as_ref()
                .map(|relative| args.results_dir.join(relative))
                .or_else(|| record.trace_path.clone());
            let metrics = match metrics_path.as_deref() {
                Some(path) => match parse_trace(path) {
                    Ok(metrics) => metrics,
                    Err(error) => {
                        attempt_issues.push(format!("cannot parse trace: {error}"));
                        Default::default()
                    }
                },
                None => Default::default(),
            };
            let expectations = verify_expectations(record.workspace.path(), spec);
            let tools = verify_tools(&metrics, spec);
            let command_checks =
                verify_command_checks(record.workspace.path(), spec, &args.python_bin);
            for check in command_checks.iter().filter(|check| !check.passed()) {
                eprintln!(
                    "{trial_prefix}L{} check `{}` — {:?}: {}",
                    spec.level,
                    check.name,
                    check.status,
                    check.reason.as_deref().unwrap_or("no reason recorded")
                );
                if check.infrastructure_error() {
                    attempt_issues.push(format!(
                        "command check `{}` infrastructure error: {}",
                        check.name,
                        check.reason.as_deref().unwrap_or("no reason recorded")
                    ));
                }
            }
            let done = completed(
                record.timed_out,
                record.exit_code,
                &expectations,
                &tools,
                &command_checks,
                metrics.terminator.as_deref(),
            );
            let attempt_finished_at = now_unix_ms();
            for message in &attempt_issues {
                issues.push(RunIssue {
                    trial_id: Some(trial_id.clone()),
                    level: Some(spec.level),
                    message: message.clone(),
                });
            }

            let row = ResultRow {
                budget: Some(budget),
                run_id: Some(run_id.clone()),
                trial_id: Some(trial_id.clone()),
                started_at_unix_ms: Some(attempt_started_at),
                finished_at_unix_ms: Some(attempt_finished_at),
                trace_path: retained_trace,
                infrastructure_error: (!attempt_issues.is_empty())
                    .then(|| attempt_issues.join("; ")),
                level: spec.level,
                spec_version: spec.version,
                level_name: spec.name.clone(),
                variant: args.variant.clone(),
                protocol: ferric_core::protocol_key(protocol),
                model: model_name.clone(),
                completed: done,
                timed_out: record.timed_out,
                exit_code: record.exit_code,
                turns: metrics.turns,
                input_tokens: metrics.input_tokens,
                output_tokens: metrics.output_tokens,
                wall_ms: record.wall.as_millis() as u64,
                terminator: metrics.terminator.clone(),
                tier_observed: metrics.tier.clone(),
                protocol_observed: metrics.protocol.clone(),
                repetition_guard_fires: metrics.repetition_guard_fires,
                tools_called: metrics.tools_called.clone(),
                task_complete_summary: metrics.task_complete_summary.clone(),
                failure_admission: failure_admission(&metrics),
                plan_steps: metrics.plan_steps,
                expectations_ok: expectations.iter().all(|expectation| expectation.passed),
                tools_ok: tools.ok(),
                command_checks,
                tier_from_params: format!("{:?}", tier_for_params(args.params_b)),
                stderr_tail: record.stderr_tail.clone(),
            };
            let row_written = match append_row(&args.results_dir, &row) {
                Ok(()) => true,
                Err(error) => {
                    let message = format!("cannot append results row: {error}");
                    eprintln!("{trial_prefix}L{} {message}", spec.level);
                    issues.push(RunIssue {
                        trial_id: Some(trial_id.clone()),
                        level: Some(spec.level),
                        message,
                    });
                    false
                }
            };
            println!(
                "{trial_prefix}L{} {} — {} ({} turns, {} tok, {} ms){}",
                spec.level,
                spec.name,
                trial_outcome_label(&row, row_written),
                row.turns,
                row.output_tokens,
                row.wall_ms,
                record
                    .workspace
                    .path()
                    .display()
                    .to_string()
                    .pipe_kept(args.keep_workspace),
            );
            let evidence_path = row
                .budget
                .as_ref()
                .and_then(|budget| budget.retained.as_ref())
                .map(|reference| args.results_dir.join(&reference.sidecar_path))
                .unwrap_or_else(|| {
                    args.results_dir.join(if row_written {
                        "results.jsonl".to_string()
                    } else {
                        format!("summary-{run_id}.json")
                    })
                });
            println!("  evidence destination: {}", evidence_path.display());
            rows.push(row);
        }
    }

    let finished_at_unix_ms = now_unix_ms();
    let expected_levels: Vec<u8> = selected.iter().map(|spec| spec.level).collect();
    let mut summary = summarize_run(
        &run_id,
        started_at_unix_ms,
        finished_at_unix_ms,
        args.trials,
        args.min_pass_rate,
        &expected_levels,
        full_ladder,
        &rows,
        issues,
        provenance(inv, args),
    );
    if summary.budget.is_none() {
        summary.budget = Some(RunBudgetEvidence {
            controls: budgets.first().map(|budget| budget.controls().clone()),
            attempts: Vec::new(),
        });
    }
    summary
        .calibration
        .record_budget_controls(budgets.first().map(|budget| budget.controls().clone()));
    if summary.calibration.is_diagnostic() {
        println!(
            "diagnostic budgets — profile left unchanged ({}); observations only",
            summary
                .calibration
                .ineligible_reason
                .as_deref()
                .unwrap_or("modified budget controls"),
        );
    }
    let summary_written = match write_summary(&args.results_dir, &summary) {
        Ok(path) => {
            println!("summary: {}", path.display());
            true
        }
        Err(error) => {
            eprintln!("cannot write benchmark summary: {error}");
            false
        }
    };
    let qualified = summary.complete && summary.levels.iter().all(|level| level.qualified);
    BenchRunOutcome {
        qualified,
        infrastructure_ok: summary.infrastructure_clean && summary_written,
        summary,
    }
}

fn provenance(inv: &Invocation, args: &BenchArgs) -> RunProvenance {
    let binary_path =
        std::fs::canonicalize(&inv.ferric_bin).unwrap_or_else(|_| inv.ferric_bin.clone());
    let metadata = std::fs::metadata(&binary_path).ok();
    let binary = BinaryProvenance {
        path: binary_path.display().to_string(),
        size_bytes: metadata.as_ref().map(std::fs::Metadata::len),
        modified_at_unix_ms: metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_unix_ms),
        sha256: ferric_bench::sha256_file(&binary_path).ok(),
    };
    let model = match &inv.openai {
        Some(openai) => ModelProvenance {
            backend: "openai".to_string(),
            model: Some(openai.model.clone()),
            api_base: openai.api_base.clone(),
            params_b: openai.params_b,
            ctx: openai.ctx,
            sha256: args.model_sha256.clone(),
        },
        None => ModelProvenance {
            backend: "mock".to_string(),
            model: None,
            api_base: None,
            params_b: args.params_b,
            ctx: args.ctx,
            sha256: None,
        },
    };
    RunProvenance {
        ferric_version: env!("CARGO_PKG_VERSION").to_string(),
        git_commit: option_env!("FERRIC_GIT_COMMIT")
            .or(option_env!("VERGEN_GIT_SHA"))
            .or(option_env!("GITHUB_SHA"))
            .map(str::to_string),
        binary,
        model,
        protocol: ferric_core::protocol_key(inv.protocol),
        variant: args.variant.clone(),
        python_bin: args.python_bin.display().to_string(),
    }
}

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_run_id(started_at_unix_ms: u64) -> String {
    let sequence = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("run-{started_at_unix_ms}-{}-{sequence}", std::process::id())
}

fn now_unix_ms() -> u64 {
    system_time_unix_ms(SystemTime::now()).unwrap_or_default()
}

fn system_time_unix_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn parse_pass_rate(value: &str) -> Result<f64, String> {
    let rate: f64 = value
        .parse()
        .map_err(|_| "pass rate must be a number greater than 0 and at most 1".to_string())?;
    if rate.is_finite() && rate > 0.0 && rate <= 1.0 {
        Ok(rate)
    } else {
        Err("pass rate must be a finite number greater than 0 and at most 1".to_string())
    }
}

fn parse_sha256(value: &str) -> Result<String, String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("SHA-256 must contain exactly 64 hexadecimal characters".to_string())
    }
}

/// Small display helper for the kept-workspace suffix.
trait PipeKept {
    fn pipe_kept(self, kept: bool) -> String;
}
impl PipeKept for String {
    fn pipe_kept(self, kept: bool) -> String {
        if kept {
            format!(" [workspace kept: {self}]")
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deliberately synthetic full-ladder success/failure. This exercises the
    // real publication decision without pretending that the mock passed L0.
    fn synthetic_summary(passed: bool, controls: BudgetControls) -> RunSummary {
        let specs = embedded_specs().unwrap();
        let rows: Vec<_> = specs
            .iter()
            .map(|spec| ResultRow {
                run_id: Some("synthetic-run".into()),
                trial_id: Some("trial-001".into()),
                started_at_unix_ms: Some(1),
                finished_at_unix_ms: Some(2),
                trace_path: None,
                budget: Some(
                    controls
                        .resolve_agent(spec.timeout_s)
                        .unwrap()
                        .evidence(Some(0), false),
                ),
                infrastructure_error: None,
                level: spec.level,
                spec_version: spec.version,
                level_name: spec.name.clone(),
                variant: "synthetic-publication-only".into(),
                protocol: "ConstrainedJson".into(),
                model: Some("synthetic-model".into()),
                completed: passed,
                timed_out: false,
                exit_code: Some(0),
                turns: 1,
                input_tokens: 1,
                output_tokens: 1,
                wall_ms: 1,
                terminator: Some("task_complete".into()),
                tier_observed: None,
                protocol_observed: None,
                repetition_guard_fires: 0,
                tools_called: Vec::new(),
                task_complete_summary: None,
                failure_admission: None,
                plan_steps: None,
                expectations_ok: passed,
                tools_ok: passed,
                command_checks: Vec::new(),
                tier_from_params: "Small".into(),
                stderr_tail: String::new(),
            })
            .collect();
        summarize_run(
            "synthetic-run",
            1,
            2,
            1,
            1.0,
            &[0, 1, 2, 3, 4, 5, 6],
            true,
            &rows,
            Vec::new(),
            RunProvenance {
                ferric_version: "synthetic".into(),
                git_commit: None,
                binary: BinaryProvenance {
                    path: "synthetic-no-process".into(),
                    size_bytes: None,
                    modified_at_unix_ms: None,
                    sha256: None,
                },
                model: ModelProvenance {
                    backend: "synthetic".into(),
                    model: Some("synthetic-model".into()),
                    api_base: None,
                    params_b: 7.0,
                    ctx: 4096,
                    sha256: None,
                },
                protocol: "ConstrainedJson".into(),
                variant: "synthetic-publication-only".into(),
                python_bin: "not-executed".into(),
            },
        )
    }

    fn default_controls() -> BudgetControls {
        BudgetControls::new(1.0, None, 7.0, 4096).unwrap()
    }

    #[test]
    fn diagnostic_single_fleet_preserve_profile_bytes() {
        let default = synthetic_summary(true, default_controls());
        assert!(default.calibration.eligible);
        assert_eq!(default.calibration.measured_level, Some(6));
        let target =
            calibrate_from_evidence("diagnostic-a", 7.0, "ConstrainedJson", &default.calibration)
                .unwrap();
        let unrelated =
            calibrate_from_evidence("unrelated", 7.0, "ConstrainedJson", &default.calibration)
                .unwrap();
        let snapshots = [
            None,
            Some(serde_json::to_vec(&vec![target.clone()]).unwrap()),
            Some(serde_json::to_vec(&vec![target, unrelated]).unwrap()),
            Some(b"{malformed profile bytes must survive".to_vec()),
        ];
        for (scale, cap) in [
            (0.5, None),
            (2.0, None),
            (1.0, Some(1024)),
            (0.5, Some(512)),
        ] {
            for passed in [false, true] {
                let summary =
                    synthetic_summary(passed, BudgetControls::new(scale, cap, 7.0, 4096).unwrap());
                assert!(summary.complete && summary.infrastructure_clean);
                assert_eq!(summary.levels.iter().all(|level| level.qualified), passed);
                assert_eq!(summary.calibration.measured_level, None);
                for models in [&["diagnostic-a"][..], &["diagnostic-a", "diagnostic-b"][..]] {
                    for snapshot in &snapshots {
                        let root = tempfile::tempdir().unwrap();
                        let results = root.path().join("results");
                        let profile = results.join("model_profiles.json");
                        if let Some(bytes) = snapshot {
                            std::fs::create_dir(&results).unwrap();
                            std::fs::write(&profile, bytes).unwrap();
                        }
                        for model in models {
                            assert!(matches!(
                                publish_calibration(
                                    &results,
                                    model,
                                    7.0,
                                    "ConstrainedJson",
                                    &summary.calibration
                                )
                                .unwrap(),
                                ProfilePublication::Diagnostic
                            ));
                            assert_eq!(std::fs::read(&profile).ok().as_ref(), snapshot.as_ref());
                            if snapshot.is_none() {
                                assert!(
                                    !results.exists(),
                                    "diagnostic publication must have no store effects"
                                );
                            }
                        }
                    }
                }
            }
        }
        // A non-file store is also deliberately never opened by this path.
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("model_profiles.json");
        std::fs::create_dir(&profile).unwrap();
        std::fs::write(profile.join("sentinel"), b"untouched").unwrap();
        let diagnostic =
            synthetic_summary(true, BudgetControls::new(2.0, None, 7.0, 4096).unwrap());
        assert!(matches!(
            publish_calibration(
                root.path(),
                "model",
                7.0,
                "ConstrainedJson",
                &diagnostic.calibration
            )
            .unwrap(),
            ProfilePublication::Diagnostic
        ));
        assert_eq!(
            std::fs::read(profile.join("sentinel")).unwrap(),
            b"untouched"
        );
    }

    #[test]
    fn default_budget_calibration_compatible() {
        let summary = synthetic_summary(true, default_controls());
        let root = tempfile::tempdir().unwrap();
        for model in ["single", "fleet-a", "fleet-b"] {
            let ProfilePublication::Published(record) = publish_calibration(
                root.path(),
                model,
                7.0,
                "ConstrainedJson",
                &summary.calibration,
            )
            .unwrap() else {
                panic!("default complete evidence must still publish");
            };
            assert_eq!(record.measured_level, Some(6));
            assert_eq!(record.tier_from_measured.as_deref(), Some("Large"));
            assert_eq!(
                ferric_bench::read_profile(root.path(), model, "ConstrainedJson"),
                Some(*record)
            );
        }
        let before = std::fs::read(root.path().join("model_profiles.json")).unwrap();
        let mut partial = summary.calibration;
        partial.full_ladder = false;
        partial.eligible = false;
        partial.ineligible_reason = Some("partial ladder".into());
        assert!(matches!(
            publish_calibration(root.path(), "single", 7.0, "ConstrainedJson", &partial).unwrap(),
            ProfilePublication::Ineligible(_)
        ));
        assert_eq!(
            std::fs::read(root.path().join("model_profiles.json")).unwrap(),
            before
        );
    }
}
