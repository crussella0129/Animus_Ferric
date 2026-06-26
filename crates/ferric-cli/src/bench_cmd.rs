//! `ferric bench` — run the L0–L6 ladder, append results.jsonl, calibrate.
//!
//! Spawns this same binary's `query` subcommand per level (release profile
//! required for usable speed — warns under debug). `--mock` is the CI-runnable
//! self-test path (no model needed).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use ferric_bench::{
    BenchSpec, Invocation, ModelArgs, ModelProfileRecord, OpenAiArgs, ResultRow, append_row,
    calibrate, completed, embedded_specs, failure_admission, parse_trace, run_spec,
    verify_expectations, verify_tools,
};
use ferric_core::{ActionProtocol, tier_for_params};

use crate::backend::BackendArg;
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

    /// Inference backend (without `--mock`): `mistral` (GGUF, `--model-dir`/
    /// `--model-file`) or `openai` (ollama / llama-server via `--api-base`/
    /// `--model` — the constrained workhorse; mistral constrained hangs, ADR-027).
    #[arg(long, value_enum, default_value = "mistral")]
    pub backend: BackendArg,

    /// Model directory (mistral backend; omit for --mock).
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// GGUF file name inside --model-dir (mistral backend).
    #[arg(long)]
    pub model_file: Option<String>,

    /// OpenAI-compatible base URL (openai backend; omit to auto-discover a
    /// running `ferric server`).
    #[arg(long)]
    pub api_base: Option<String>,

    /// Model identifier for the openai backend (e.g. `qwen2.5-coder:7b`).
    #[arg(long)]
    pub model: Option<String>,

    /// Fleet sweep (openai backend): run the full L0–L6 ladder for each
    /// comma-separated model id and print a `measured_level` leaderboard. One
    /// profile record per model. Overrides `--model`.
    #[arg(long)]
    pub models: Option<String>,

    #[arg(long, default_value_t = 1.2)]
    pub params_b: f32,

    #[arg(long, default_value_t = 4096)]
    pub ctx: u32,

    /// Prompt-element library passed to each run.
    #[arg(long)]
    pub prompts_dir: Option<PathBuf>,

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

    let protocol: ActionProtocol = args.protocol.into();
    let ferric_bin = args
        .ferric_bin
        .clone()
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("ferric"));

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
        for model_id in &model_ids {
            println!("\n=== {model_id} ===");
            let inv = Invocation {
                ferric_bin: ferric_bin.clone(),
                protocol,
                model: None,
                openai: Some(OpenAiArgs {
                    api_base: args.api_base.clone(),
                    model: model_id.clone(),
                    params_b: args.params_b,
                    ctx: args.ctx,
                }),
                prompts_dir: args.prompts_dir.clone(),
                keep_workspace: args.keep_workspace,
            };
            let (rows, _) = run_levels(&selected, &inv, protocol, &Some(model_id.clone()), &args);
            let record = calibrate(model_id, args.params_b, &format!("{protocol:?}"), &rows);
            if let Err(e) = ferric_bench::write_profile(&args.results_dir, &record) {
                eprintln!("cannot write model profile: {e}");
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
        return ExitCode::SUCCESS;
    }

    // Single-model path. openai = ollama/llama-server; mistral = GGUF; `model_name`
    // keys the calibration record (model-file for mistral, model-id for openai).
    let (model, openai, model_name) = if args.mock {
        (None, None, None)
    } else {
        match args.backend {
            BackendArg::Openai => {
                let Some(model_id) = args.model.clone() else {
                    eprintln!("--model <id> is required for --backend openai");
                    return ExitCode::FAILURE;
                };
                let oa = OpenAiArgs {
                    api_base: args.api_base.clone(),
                    model: model_id.clone(),
                    params_b: args.params_b,
                    ctx: args.ctx,
                };
                (None, Some(oa), Some(model_id))
            }
            BackendArg::Mistral => match (&args.model_dir, &args.model_file) {
                (Some(dir), Some(file)) => {
                    let ma = ModelArgs {
                        model_dir: dir.clone(),
                        model_file: file.clone(),
                        params_b: args.params_b,
                        ctx: args.ctx,
                    };
                    (Some(ma), None, Some(file.clone()))
                }
                _ => {
                    eprintln!("--model-dir and --model-file are required for --backend mistral");
                    return ExitCode::FAILURE;
                }
            },
        }
    };
    let inv = Invocation {
        ferric_bin,
        protocol,
        model,
        openai,
        prompts_dir: args.prompts_dir.clone(),
        keep_workspace: args.keep_workspace,
    };

    let (rows, all_passed) = run_levels(&selected, &inv, protocol, &model_name, &args);

    // Calibrate from this sweep's rows.
    if let Some(model_name) = &model_name {
        let record = calibrate(model_name, args.params_b, &format!("{protocol:?}"), &rows);
        if let Err(e) = ferric_bench::write_profile(&args.results_dir, &record) {
            eprintln!("cannot write model profile: {e}");
        } else if let Some(level) = record.measured_level {
            println!(
                "calibrated {model_name}: measured_level {level} ({} -> {})",
                record.tier_from_params,
                record.tier_from_measured.as_deref().unwrap_or("?")
            );
        }
    }

    if all_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Run the full ladder for one (model, `inv`): execute each spec, verify, append
/// a results row, print PASS/FAIL, and return the rows + whether every level
/// passed. Shared by the single-model and `--models` fleet paths.
fn run_levels(
    selected: &[BenchSpec],
    inv: &Invocation,
    protocol: ActionProtocol,
    model_name: &Option<String>,
    args: &BenchArgs,
) -> (Vec<ResultRow>, bool) {
    let mut rows: Vec<ResultRow> = Vec::new();
    let mut all_passed = true;
    for spec in selected {
        let record = match run_spec(spec, inv) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("L{} run error: {e}", spec.level);
                all_passed = false;
                continue;
            }
        };
        let metrics = record
            .trace_path
            .as_deref()
            .and_then(|p| parse_trace(p).ok())
            .unwrap_or_default();
        let expectations = verify_expectations(record.workspace.path(), spec);
        let tools = verify_tools(&metrics, spec);
        let done = completed(
            record.timed_out,
            record.exit_code,
            &expectations,
            &tools,
            metrics.terminator.as_deref(),
        );
        all_passed &= done;

        let row = ResultRow {
            level: spec.level,
            level_name: spec.name.clone(),
            variant: args.variant.clone(),
            protocol: format!("{protocol:?}"),
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
            expectations_ok: expectations.iter().all(|e| e.passed),
            tools_ok: tools.ok(),
            tier_from_params: format!("{:?}", tier_for_params(args.params_b)),
            stderr_tail: record.stderr_tail.clone(),
        };
        if let Err(e) = append_row(&args.results_dir, &row) {
            eprintln!("cannot append results row: {e}");
        }
        println!(
            "L{} {} — {} ({} turns, {} tok, {} ms){}",
            spec.level,
            spec.name,
            if done { "PASS" } else { "FAIL" },
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
    (rows, all_passed)
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
