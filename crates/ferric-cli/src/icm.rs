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

mod workspace;
#[cfg(test)]
mod workspace_tests;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use ferric_guard::Workspace;
use workspace::{ComposeMode, IcmWorkspace, plan, plan_with_mode, scaffold_workspace};

use crate::backend::{BackendOpts, ResolvedBackend};
use crate::config::{HarnessPolicyArg, RunFlags, TierArg};
use crate::query::{
    RunConfigArgs, build_run_config, create_trace_sink, mock_provider, run_with_provider,
};

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

    /// Run at this tier regardless of size or measured level (ADR-098).
    #[arg(long, value_enum)]
    tier: Option<TierArg>,

    /// Cap the active tool ring (ADR-028). Restrict-only.
    #[arg(long)]
    max_ring: Option<u8>,

    /// Autonomous harness policy for every stage in the pipeline.
    #[arg(long, value_enum)]
    harness_policy: Option<HarnessPolicyArg>,

    /// Serve Layer-3 references on demand via the `fetch_reference` tool instead
    /// of folding whole reference files into each stage's prompt (Dark Matter
    /// knowledge layer, ADR-071). Default: fold (the original behavior).
    #[arg(long)]
    fetch_references: bool,
}

pub fn run_icm(args: IcmArgs) -> ExitCode {
    crate::exit_with(match args.command {
        IcmCommand::Init { path } => run_init(&path),
        IcmCommand::Plan {
            workspace,
            show_context,
        } => run_plan(&workspace, show_context),
        IcmCommand::Run(args) => run_pipeline(&args),
    })
}

fn run_init(path: &Path) -> Result<ExitCode, String> {
    let report = scaffold_workspace(path).map_err(|e| format!("icm init failed: {e}"))?;
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
    Ok(ExitCode::SUCCESS)
}

fn run_plan(workspace: &Path, show_context: bool) -> Result<ExitCode, String> {
    let ws = IcmWorkspace::discover(workspace).map_err(|e| format!("icm plan failed: {e}"))?;
    let plan = plan(&ws).map_err(|e| format!("icm plan failed: {e}"))?;

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
    Ok(ExitCode::SUCCESS)
}

fn run_pipeline(args: &IcmRunArgs) -> Result<ExitCode, String> {
    let ws = IcmWorkspace::discover(&args.workspace).map_err(|e| format!("icm run failed: {e}"))?;
    let mode = if args.fetch_references {
        ComposeMode::FetchReferences
    } else {
        ComposeMode::FoldReferences
    };
    let plan = plan_with_mode(&ws, mode).map_err(|e| format!("icm run failed: {e}"))?;

    // Resolve the 1-based stage range against the discovered stage count.
    let total = plan.stages.len();
    let from = args.from.unwrap_or(1).max(1);
    let to = args.to.unwrap_or(total).min(total);
    if from > to {
        return Err(format!(
            "icm run: empty stage range (--from {from} > --to {to})"
        ));
    }

    // Build the run config + provider ONCE, reused for every stage (the model is
    // fixed across the pipeline; only the workspace changes per stage). Config is
    // resolved from the ICM root, exactly like `ferric query`/`ferric mcp`, for
    // the subset of run flags ICM exposes.
    let cfg = crate::config::load_layered(&ws.root)
        .map_err(|error| error.to_string())?
        .config;
    let flags = RunFlags {
        backend_opts: args.backend_opts.clone(),
        mock: args.mock,
        params_b: args.params_b,
        ctx: args.ctx,
        tier: args.tier,
        max_ring: args.max_ring,
        harness_policy: args.harness_policy,
        ..RunFlags::default()
    };
    // ICM composes each stage's context from its OWN declared layers (L0-L4,
    // ADR-064) and reports that composition as provenance. Folding skills in
    // here would inject text no layer accounts for, so ICM opts out
    // deliberately rather than by omission.
    let run = RunConfigArgs {
        hooks: cfg.hooks.clone(),
        ..flags.resolve(&cfg, &ws.root)
    };
    let mut config = build_run_config(&run).map_err(|error| error.to_string())?;

    // ICM-only: `fetch_reference` is not part of the global builtin set, so it
    // never costs a normal `ferric query` run a tool-budget slot. It joins the
    // ring only when references are served on demand (ADR-071).
    if args.fetch_references {
        config
            .registry
            .register(Box::new(ferric_tools::builtin::FetchReference));
    }

    // A real backend is stateless per request, so its provider and the one
    // runtime are built once and reused across every stage; the single-use
    // scripted mock is rebuilt per stage (each stage is its own session).
    let backend = ResolvedBackend::resolve(run.mock, &run.backend, &ws.root)
        .map_err(|e| format!("icm run failed: {e}"))?;

    // Traces centralize at the ICM root (harness-written, not agent-written, so
    // not subject to the per-stage workspace boundary) — one file per stage.
    let trace_dir = ferric_trace::trace_dir(&ws.root);
    std::fs::create_dir_all(&trace_dir).map_err(|e| {
        format!(
            "icm run: cannot create trace dir {}: {e}",
            trace_dir.display()
        )
    })?;

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
        let stage_ws = Workspace::new(&stage_dir)
            .map_err(|e| format!("✖ Stage {:02} · {}: workspace {e}", stage.index, stage.name))?;
        let (_, _, mut sink) = create_trace_sink(&trace_dir, &format!("icm-{}", stage.name))
            .map_err(|e| format!("✖ Stage {:02}: cannot open trace: {e}", stage.index))?;

        eprintln!("▶ Stage {:02} · {} — running…", stage.index, stage.name);

        let outcome = backend.with_provider(
            || mock_provider(config.protocol),
            |provider| {
                backend.block_on(run_with_provider(
                    config.run_args(&stage_ws, provider),
                    &mut sink,
                    Some(&stage.prompt),
                ))
            },
        );

        match outcome {
            Ok(o) if o.stop.is_success() => {
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
                return Err(format!(
                    "✖ Stage {:02} · {} did not complete ({}); halting pipeline.",
                    stage.index,
                    stage.name,
                    o.stop.as_str()
                ));
            }
            Err(message) => {
                return Err(format!(
                    "✖ Stage {:02} · {}: {message}",
                    stage.index, stage.name
                ));
            }
        }

        // Human review gate: pause before the next stage reads this output,
        // unless --auto or this is the last stage in the range.
        if !args.auto && n < to && !review_gate(stage.index) {
            eprintln!("Pipeline stopped at stage {:02} by user.", stage.index);
            return Ok(ExitCode::SUCCESS);
        }
    }

    println!("ICM pipeline finished.");
    Ok(ExitCode::SUCCESS)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_run_args(workspace: &Path, harness_policy: HarnessPolicyArg) -> IcmRunArgs {
        IcmRunArgs {
            workspace: workspace.to_path_buf(),
            auto: true,
            from: Some(1),
            to: Some(1),
            backend_opts: BackendOpts::default(),
            mock: true,
            params_b: None,
            ctx: None,
            tier: None,
            max_ring: None,
            harness_policy: Some(harness_policy),
            fetch_references: false,
        }
    }

    #[test]
    fn bounded_icm_stage_preserves_explicit_evidence_policy() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("pipeline");
        scaffold_workspace(&root).unwrap();

        assert_eq!(
            run_pipeline(&mock_run_args(&root, HarnessPolicyArg::Evidence)),
            Ok(ExitCode::SUCCESS)
        );

        let trace_dir = ferric_trace::trace_dir(&root);
        let traces: Vec<_> = std::fs::read_dir(trace_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(traces.len(), 1);
        let selected = ferric_trace::TraceReader::open(&traces[0])
            .unwrap()
            .find_map(|record| match record.unwrap().event {
                ferric_trace::ParsedEvent::Known(ferric_trace::Event::PolicySelected {
                    harness_policy,
                    ..
                }) => Some(harness_policy),
                _ => None,
            })
            .expect("a policy_selected event");
        assert_eq!(selected, ferric_core::HarnessPolicy::Evidence);
    }
}
