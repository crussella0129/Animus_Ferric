//! `ferric bench` — run the L0–L6 ladder, append results.jsonl, calibrate.
//!
//! Spawns this same binary's `query` subcommand per level (release profile
//! required for usable speed — warns under debug). `--mock` is the CI-runnable
//! self-test path (no model needed).

use std::path::PathBuf;
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
    #[arg(long, default_value_t = 1.0)]
    pub timeout_scale: f64,

    /// Explicit main-action output cap; constrained by declared context reserve.
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

    /// Where results.jsonl and model_profiles.json are written.
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
    if cfg!(debug_assertions) && !args.mock {
        eprintln!(
            "warning: running a real-model sweep from a DEBUG binary — inference will be ~1 tok/s. \
             Rebuild with --release."
        );
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
            let Some(record) = calibrate_from_evidence(
                model_id,
                args.params_b,
                &ferric_core::protocol_key(protocol),
                &outcome.summary.calibration,
            ) else {
                println!(
                    "{model_id}: profile left unchanged ({})",
                    outcome
                        .summary
                        .calibration
                        .ineligible_reason
                        .as_deref()
                        .unwrap_or("calibration evidence was not eligible")
                );
                continue;
            };
            if let Err(e) = ferric_bench::write_profile(&args.results_dir, &record) {
                eprintln!("cannot write model profile: {e}");
                infrastructure_ok = false;
                continue;
            }
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
        println!("\n# Agentic Capability Leaderboard (L0-L6)");
        println!("| Model | measured_level | tier |");
        println!("|-------|----------------|------|");
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
    if model_name.is_some() && !full_ladder {
        println!(
            "partial sweep ({} level(s)) — profile left unchanged; run without --level to recalibrate",
            selected.len()
        );
    }
    if let Some(model_name) = &model_name
        && full_ladder
        && infrastructure_ok
    {
        let Some(record) = calibrate_from_evidence(
            model_name,
            args.params_b,
            &ferric_core::protocol_key(protocol),
            &outcome.summary.calibration,
        ) else {
            eprintln!(
                "calibration evidence is not eligible: {}",
                outcome
                    .summary
                    .calibration
                    .ineligible_reason
                    .as_deref()
                    .unwrap_or("unknown reason")
            );
            return ExitCode::FAILURE;
        };
        if let Err(e) = ferric_bench::write_profile(&args.results_dir, &record) {
            eprintln!("cannot write model profile: {e}");
            infrastructure_ok = false;
        } else if let Some(level) = record.measured_level {
            println!(
                "calibrated {model_name}: measured_level {level} ({} -> {})",
                record.tier_from_params,
                record.tier_from_measured.as_deref().unwrap_or("?")
            );
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
                if !row_written || row.infrastructure_error.is_some() {
                    "INFRASTRUCTURE FAILURE"
                } else if done {
                    "PASS"
                } else {
                    "FAIL"
                },
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
