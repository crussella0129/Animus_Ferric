//! `ferric icm` — Interpretable Context Methodology workspace tooling
//! (ADR-064, sprint 73).
//!
//! ICM makes the *filesystem* the orchestration layer: numbered stage folders
//! run in order, each stage's `CONTEXT.md` is a contract, and a five-layer
//! hierarchy scopes what each stage-agent sees. Increment 1 surfaces the pure
//! model:
//! - `ferric icm init <path>` scaffolds a new workspace (LLM-free,
//!   refuse-to-clobber, mirroring `ferric launch`).
//! - `ferric icm plan <workspace>` prints the orchestration plan — which files,
//!   at which layers, each stage-agent would receive — with no model in the
//!   loop. This is the delegation logic made inspectable ("open the folder,
//!   read the files").
//!
//! Live per-stage execution (`ferric icm run`, feeding each composed prompt into
//! the constrained loop with human review gates) is increment 2; the plan this
//! command prints is exactly what it will execute. All work is workspace-scoped
//! through `ferric-guard`, so a contract can never reference context outside the
//! workspace.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use ferric_icm::{IcmWorkspace, plan, scaffold_workspace};

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
}

pub fn run_icm(args: IcmArgs) -> ExitCode {
    match args.command {
        IcmCommand::Init { path } => run_init(&path),
        IcmCommand::Plan {
            workspace,
            show_context,
        } => run_plan(&workspace, show_context),
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
