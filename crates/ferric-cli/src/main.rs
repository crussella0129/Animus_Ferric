//! The `ferric` binary. Surfaces (ADR-011 — no chat catch-all; revised
//! 2026-06-29 to add both `ferric mcp` and a raw chat mode, both now built):
//! - `ferric query "<prompt>"` — one-shot, workspace-scoped, policy-scaled,
//!   fully traced (T-111).
//! - `ferric mcp` — MCP-stdio server exposing one tool, `ferric_query`
//!   (ADR-046).
//! - `ferric chat` — hybrid chat REPL: unconstrained talk by default,
//!   user-initiated `/do <request>` escalation into the constrained agentic
//!   loop (ADR-052).
//! - `ferric launch` — Animus Launch (inc 1): a deterministic, LLM-free
//!   bootstrapper that scaffolds a new git repo (main+dev) + a sprint-loop
//!   skeleton from an interview or flags (ADR-053).
//! - `ferric trace cat <file>` — derived view of a JSONL trace.
//! - `ferric dev` — reserved for the Development Engine (s4–s7).

mod backend;
mod bench_cmd;
mod chat;
mod config;
mod launch;
mod mcp;
mod query;
mod server;
mod toolbench_cmd;
mod trace_cmd;
mod trace_verify;

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
    Query(Box<query::QueryArgs>),
    /// Run the L0–L6 capability benchmark and calibrate measured_level
    Bench(Box<bench_cmd::BenchArgs>),
    /// Run single-turn tool fire rate tests
    Toolbench(Box<toolbench_cmd::ToolbenchArgs>),
    /// Run the MCP-stdio server (one tool, `ferric_query`; ADR-046)
    Mcp(Box<mcp::McpArgs>),
    /// Start an interactive chat REPL (hybrid talk + `/do` escalate; ADR-052)
    Chat(Box<chat::ChatArgs>),
    /// Bootstrap a new project: scaffold a git repo (main+dev) + skeleton (ADR-053)
    Launch(Box<launch::LaunchArgs>),
    /// Launch and manage the OpenAI-compatible inference server (the HTTP valve)
    Server {
        #[command(subcommand)]
        command: server::ServerCommand,
    },
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
    /// Replay a trace file with a MockProvider to ensure execution hasn't drifted
    Verify { golden: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Query(args) => query::run_query(*args),
        Command::Bench(args) => bench_cmd::run_bench(*args),
        Command::Toolbench(args) => toolbench_cmd::run_toolbench(*args),
        Command::Mcp(args) => mcp::run_mcp(*args),
        Command::Chat(args) => chat::run_chat(*args),
        Command::Launch(args) => launch::run_launch(*args),
        Command::Server { command } => {
            let workspace = std::env::current_dir().unwrap_or_default();
            server::run_server(&workspace, command)
        }
        Command::Trace {
            command: TraceCommand::Cat { file },
        } => trace_cmd::trace_cat(&file),
        Command::Trace {
            command: TraceCommand::Verify { golden },
        } => trace_verify::trace_verify(&golden),
    }
}
