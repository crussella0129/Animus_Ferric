//! `ferric icm` — Interpretable Context Methodology workspace tooling
//! (ADR-064/ADR-065, sprints 73–74).
//!
//! ICM makes the *filesystem* the orchestration layer: numbered stage folders
//! run in order, each stage's `CONTEXT.md` is a contract, and a five-layer
//! hierarchy scopes what each stage-agent sees.
//!
//! - `ferric icm init <path>` scaffolds a new workspace (LLM-free,
//!   refuse-to-clobber, mirroring `ferric launch`).
//! - `ferric icm plan <workspace>` prints the orchestration plan — which files,
//!   at which layers, each stage-agent would receive — with no model in the
//!   loop. The delegation logic made inspectable.
//! - `ferric icm run <workspace>` **executes** the pipeline: each stage's
//!   composed context is fed into the same constrained agent loop `ferric query`
//!   drives (`run_with_provider`), in numeric order, with a human review gate
//!   between stages (`--auto` runs straight through). A stage that does not reach
//!   a successful terminator halts the pipeline (downstream stages depend on it).
//!
//! **Security — the Ferric-native adaptation.** Each stage executes contained to
//! its OWN folder (`Workspace::new(stages/NN_*)`), stronger than the paper's
//! by-convention model: a stage can only write inside its own directory, so it
//! cannot clobber a sibling stage or the shared config. Cross-stage data flows
//! only through the composed context (which already folds the prior stage's
//! output in as Layer 4) and the human review gate — never through unrestricted
//! filesystem access.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use ferric_guard::Workspace;
use ferric_icm::{IcmWorkspace, plan, scaffold_workspace};
use ferric_loop::StopReason;
// Only the feature-gated real backend names the `Provider` trait object.
#[cfg(feature = "backend-openai")]
use ferric_provider::Provider;
use ferric_trace::JsonlSink;

use crate::backend::{BackendArg, BackendOpts};
use crate::query::{RunConfigArgs, build_run_config, mock_provider, now_ms, run_with_provider};

#[derive(Args)]
pub struct IcmArgs {
    #[command(subcommand)]
    command: IcmCommand,
}

#[derive(Subcommand)]
enum IcmCommand {
    /// Scaffold a new ICM workspace skeleton (LLM-free, refuse-to-clobber)
    Init {
        /// Directory to create the workspace in
        path: PathBuf,
    },
    /// Show the orchestration plan: each stage's scoped context and provenance
    Plan {
        /// The ICM workspace root
        workspace: PathBuf,
        /// Also print the full composed prompt text for each stage
        #[arg(long)]
        show_context: bool,
    },
    /// Execute the pipeline: run each stage's composed context through the
    /// constrained agent loop, in order, with review gates between stages
    Run(Box<IcmRunArgs>),
}

#[derive(Args)]
pub struct IcmRunArgs {
    /// The ICM workspace root
    workspace: PathBuf,

    /// Run every stage straight through with no human review gate between them
    #[arg(long)]
    auto: bool,

    /// First stage to run (1-based, inclusive). Default: the first stage.
    #[arg(long)]
    from: Option<usize>,

    /// Last stage to run (1-based, inclusive). Default: the last stage.
    #[arg(long)]
    to: Option<usize>,

    #[command(flatten)]
    backend_opts: BackendOpts,

    /// Run each stage against the built-in scripted mock instead of a model
    #[arg(long)]
    mock: bool,

    /// Parameter count in billions (selects tier/protocol). Default 1.2.
    #[arg(long)]
    params_b: Option<f32>,

    /// Context window in tokens. Default 4096.
    #[arg(long)]
    ctx: Option<u32>,

    /// Cap the active tool ring (ADR-028). Restrict-only.
    #[arg(long)]
    max_ring: Option<u8>,
}

pub fn run_icm(args: IcmArgs) -> ExitCode {
    match args.command {
        IcmCommand::Init { path } => run_init(&path),
        IcmCommand::Plan {
            workspace,
            show_context,
        } => run_plan(&workspace, show_context),
        IcmCommand::Run(args) => run_pipeline(*args),
    }
}

fn run_init(path: &Path) -> ExitCode {
    match scaffold_workspace(path) {
        Ok(report) => {
            println!("Scaffolded ICM workspace at {}", report.root.display());
            println!(
                "  {} directories, {} files",
                report.dirs_created.len(),
                report.files_created.len()
            );
            println!("Stages: 01_research -> 02_script -> 03_production");
            println!(
                "Next: edit the stage CONTEXT.md contracts, then `ferric icm plan {}`.",
                path.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("icm init failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_plan(workspace: &Path, show_context: bool) -> ExitCode {
    let ws = match IcmWorkspace::discover(workspace) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("icm plan failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let plan = match plan(&ws) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("icm plan failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("ICM orchestration plan for {}", ws.root.display());
    println!(
        "{} stage(s), executed in numeric order:\n",
        plan.stages.len()
    );

    for stage in &plan.stages {
        println!("── Stage {:02} · {}", stage.index, stage.name);
        for p in &stage.provenance {
            let mark = if p.present { " " } else { "!" };
            let note = if p.present {
                format!("{} bytes", p.bytes)
            } else {
                "MISSING".to_string()
            };
            println!(
                "   {mark} L{} {:<22} {}  ({note})",
                p.layer, p.label, p.source
            );
        }
        if show_context {
            println!("   ┌─ composed context ───────────────────────────");
            for line in stage.prompt.lines() {
                println!("   │ {line}");
            }
            println!("   └──────────────────────────────────────────────");
        }
        println!();
    }

    // A '!' marker means a declared input is absent (e.g. an upstream stage has
    // not run yet). That is expected pre-run, not an error.
    ExitCode::SUCCESS
}

/// The pipeline's backend. The scripted mock is single-use, so a **fresh** one
/// is built per stage (each stage is its own agent session — mirrors `ferric
/// query` and the chat REPL's per-turn mock). A real backend is stateless per
/// request, so its provider and the one tokio runtime are built once and reused
/// across every stage.
enum Backend {
    Mock,
    #[cfg(feature = "backend-openai")]
    Real {
        provider: Box<dyn Provider + Send + Sync>,
        runtime: tokio::runtime::Runtime,
    },
}

fn run_pipeline(args: IcmRunArgs) -> ExitCode {
    let ws = match IcmWorkspace::discover(&args.workspace) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("icm run failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let plan = match plan(&ws) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("icm run failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Resolve the 1-based stage range against the discovered stage count.
    let total = plan.stages.len();
    let from = args.from.unwrap_or(1).max(1);
    let to = args.to.unwrap_or(total).min(total);
    if from > to {
        eprintln!("icm run: empty stage range (--from {from} > --to {to})");
        return ExitCode::FAILURE;
    }

    // Build the run config + provider ONCE, reused for every stage (the model is
    // fixed across the pipeline; only the workspace changes per stage). Config is
    // resolved from the ICM root, exactly like `ferric query`/`ferric mcp`.
    let loaded = crate::config::load_layered(&ws.root);
    let cfg = loaded.config;
    let backend_opts = crate::config::merge_backend_opts(args.backend_opts.clone(), &cfg);
    for diag in &loaded.diagnostics {
        eprintln!("{diag}");
    }
    let config = build_run_config(&RunConfigArgs {
        mock: args.mock,
        backend: backend_opts.backend.unwrap_or(BackendArg::Openai),
        params_b: args.params_b.or(cfg.params_b).unwrap_or(1.2),
        quant: cfg.quant.clone().unwrap_or_else(|| "Q4_K_M".to_string()),
        family: cfg.family.clone().unwrap_or_else(|| "unknown".to_string()),
        ctx: args.ctx.or(cfg.ctx).unwrap_or(4096),
        temperature: cfg.temperature.unwrap_or(0.0),
        protocol_override: None,
        prompts_dir: None,
        max_ring: args.max_ring.or(cfg.max_ring),
        profile_dir: cfg
            .profile_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("benchmarks")),
        model_key: backend_opts.model.clone(),
        hooks: cfg.hooks.clone(),
    });

    let backend = if args.mock {
        Backend::Mock
    } else {
        match build_real_backend(&backend_opts) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("icm run failed: {e}");
                return ExitCode::FAILURE;
            }
        }
    };

    // Traces centralize at the ICM root (harness-written, not agent-written, so
    // not subject to the per-stage workspace boundary) — one file per stage.
    let trace_dir = ws.root.join(".ferric").join("trace");
    if let Err(e) = std::fs::create_dir_all(&trace_dir) {
        eprintln!(
            "icm run: cannot create trace dir {}: {e}",
            trace_dir.display()
        );
        return ExitCode::FAILURE;
    }

    println!(
        "Running ICM pipeline: stages {from}–{to} of {total} in {}",
        ws.root.display()
    );

    for (i, stage) in plan.stages.iter().enumerate() {
        let n = i + 1;
        if n < from || n > to {
            continue;
        }

        let stage_dir = ws.root.join("stages").join(&stage.name);
        let stage_ws = match Workspace::new(&stage_dir) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("✖ Stage {:02} · {}: workspace {e}", stage.index, stage.name);
                return ExitCode::FAILURE;
            }
        };

        let session = format!("icm-{}-{}", stage.name, now_ms());
        let trace_path = trace_dir.join(format!("{session}.jsonl"));
        let mut sink = match JsonlSink::open(&trace_path, &session) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("✖ Stage {:02}: cannot open trace: {e}", stage.index);
                return ExitCode::FAILURE;
            }
        };

        eprintln!("▶ Stage {:02} · {} — running…", stage.index, stage.name);

        // Same run for either backend — only the provider ref and executor
        // differ, so the (long) argument list lives in one macro.
        macro_rules! run_stage {
            ($provider:expr) => {
                run_with_provider(
                    $provider,
                    &config.registry,
                    &stage_ws,
                    &config.policy,
                    config.protocol,
                    config.sampling.clone(),
                    config.system_prompt.as_deref(),
                    config.lineage.clone(),
                    &mut sink,
                    Some(&stage.prompt),
                    Vec::new(),
                    None,
                    None,
                    ferric_guard::TaintSet::new(),
                    ferric_guard::SinkPolicy::deny(),
                    None,
                    config.hooks.clone(),
                    None,
                )
            };
        }
        let outcome = match &backend {
            Backend::Mock => {
                let provider = mock_provider(config.protocol);
                futures_executor::block_on(run_stage!(&provider))
            }
            #[cfg(feature = "backend-openai")]
            Backend::Real { provider, runtime } => runtime.block_on(run_stage!(provider.as_ref())),
        };

        match outcome {
            Ok(o) if is_success(o.stop) => {
                eprintln!(
                    "✔ Stage {:02} · {} complete ({}, {} turn(s)). Output in stages/{}/output/",
                    stage.index,
                    stage.name,
                    o.stop.as_str(),
                    o.turns,
                    stage.name
                );
                if let Some(text) = &o.final_text {
                    println!("[{}] {text}", stage.name);
                }
            }
            Ok(o) => {
                eprintln!(
                    "✖ Stage {:02} · {} did not complete ({}); halting pipeline.",
                    stage.index,
                    stage.name,
                    o.stop.as_str()
                );
                return ExitCode::FAILURE;
            }
            Err(message) => {
                eprintln!("✖ Stage {:02} · {}: {message}", stage.index, stage.name);
                return ExitCode::FAILURE;
            }
        }

        // Human review gate: pause before the next stage reads this output,
        // unless --auto or this is the last stage in the range.
        if !args.auto && n < to && !review_gate(stage.index) {
            eprintln!("Pipeline stopped at stage {:02} by user.", stage.index);
            return ExitCode::SUCCESS;
        }
    }

    println!("ICM pipeline finished.");
    ExitCode::SUCCESS
}

/// A successful terminator: the stage produced its deliverable and stopped
/// cleanly. Any other stop (max turns, provider error, guard trip) means the
/// stage's output cannot be trusted as input to the next stage.
fn is_success(stop: StopReason) -> bool {
    matches!(
        stop,
        StopReason::TaskComplete | StopReason::PlanSubmitted | StopReason::FinalText
    )
}

/// Prompt on stderr and read a line from stdin. Returns `true` to continue,
/// `false` if the user typed `q`/`quit`. EOF (closed stdin) continues, so a
/// non-interactive run without `--auto` does not hang.
fn review_gate(stage_index: u32) -> bool {
    eprint!(
        "   next stage waits: review this output, then [Enter] to continue or 'q' to stop (after stage {stage_index:02}): "
    );
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => true, // EOF: no interactive reviewer; proceed.
        Ok(_) => !matches!(line.trim().to_ascii_lowercase().as_str(), "q" | "quit"),
        Err(_) => true,
    }
}

#[cfg(feature = "backend-openai")]
fn build_real_backend(backend_opts: &BackendOpts) -> Result<Backend, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    let provider = runtime.block_on(crate::backend::create_provider(backend_opts))?;
    Ok(Backend::Real { provider, runtime })
}

#[cfg(not(feature = "backend-openai"))]
fn build_real_backend(_backend_opts: &BackendOpts) -> Result<Backend, String> {
    Err("this binary was built without backend features; \
         rebuild with `cargo build --features backend-openai`, or use --mock"
        .to_string())
}
