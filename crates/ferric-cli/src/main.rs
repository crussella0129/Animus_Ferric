//! The `ferric` binary. Surfaces (ADR-011 — no chat catch-all):
//! - `ferric query "<prompt>"` — one-shot, workspace-scoped, policy-scaled,
//!   fully traced (T-111).
//! - `ferric trace cat <file>` — derived view of a JSONL trace.
//! - `ferric dev` — reserved for the Development Engine (s4–s7).

mod query;
mod trace_cmd;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ferric",
    version,
    about = "Animus Ferric: a local-first agentic coding harness for small models"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a one-shot, workspace-scoped query against a local model
    Query(query::QueryArgs),
    /// Inspect session traces
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },
}

#[derive(Subcommand)]
enum TraceCommand {
    /// Render a JSONL trace as a human-readable log
    Cat { file: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Query(args) => query::run_query(args),
        Command::Trace {
            command: TraceCommand::Cat { file },
        } => trace_cmd::trace_cat(&file),
    }
}
