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
//! - `ferric api` — HTTP API server for IDE/web/mobile integration
//!   (Sprint 64).
//! - `ferric trace cat <file>` — derived view of a JSONL trace.
//! - `ferric dev` — reserved for the Development Engine (s4–s7).

mod api;
mod autonomy_cmd;
mod backend;
mod bench_cmd;
mod chat;
mod config;
mod cron;
mod dream_cmd;
mod icm;
mod launch;
mod logging;
mod mcp;
mod query;
mod revert_cmd;
mod server;
mod server_process;
mod server_registration;
mod server_resolution;
mod skills_cmd;
mod tailscale_serve;
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
    /// Increase harness-internal log verbosity (`-v` info, `-vv` debug,
    /// `-vvv` trace). Diagnostics go to stderr — stdout stays a clean machine
    /// channel. `FERRIC_LOG`/`RUST_LOG` override this entirely (ADR-063).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a one-shot, workspace-scoped query against a local model
    Query(Box<query::QueryArgs>),
    /// Run benchmarking suites (ltd for syntax fire-rate, full for L0-L6 ladder)
    Bench {
        #[command(subcommand)]
        command: BenchCommand,
    },
    /// Run the MCP-stdio server (one tool, `ferric_query`; ADR-046)
    Mcp(Box<mcp::McpArgs>),
    /// Start an interactive chat REPL (hybrid talk + `/do` escalate; ADR-052)
    Chat(Box<chat::ChatArgs>),
    /// Bootstrap a new project: scaffold a git repo (main+dev) + skeleton (ADR-053)
    Launch(Box<launch::LaunchArgs>),
    /// ICM agent delegation: scaffold/plan a filesystem-orchestrated workspace (ADR-064)
    Icm(Box<icm::IcmArgs>),
    /// Agentic cron: schedule periodic agent tasks from `.ferric/cron/` (ADR-066)
    Cron(Box<cron::CronArgs>),
    /// Start the HTTP API server for IDE/web/mobile integration (Sprint 64)
    #[cfg(feature = "backend-openai")]
    Api(Box<api::server::ApiArgs>),
    /// Revert the workspace and trace to a specific turn snapshot
    Revert(Box<revert_cmd::RevertArgs>),
    /// Dream Mode: Asynchronously parse historical traces and extract knowledge
    Dream(Box<dream_cmd::DreamArgs>),
    /// Launch and manage the OpenAI-compatible inference server (the HTTP valve)
    Server {
        #[command(subcommand)]
        command: server::ServerCommand,
    },
    /// Inspect installed agent skills
    Skills {
        #[command(subcommand)]
        command: skills_cmd::SkillsCommand,
    },
    /// Inspect session traces
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },
}

#[derive(Subcommand)]
enum BenchCommand {
    /// Run single-turn tool fire rate tests
    Ltd(Box<toolbench_cmd::ToolbenchArgs>),
    /// Run the L0–L6 capability benchmark and calibrate measured_level
    Full(Box<bench_cmd::BenchArgs>),
    /// Run the versioned internal autonomous repository-work matrix
    Autonomy(Box<autonomy_cmd::AutonomyArgs>),
}

#[derive(Subcommand)]
enum TraceCommand {
    /// Render a JSONL trace as a human-readable log
    Cat { file: PathBuf },
    /// Validate trace structure without re-executing recorded tool calls
    Verify { golden: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Install the stderr diagnostic subscriber before any command runs, so
    // every subcommand (and its libraries) emits through one configured sink.
    logging::init(cli.verbose);
    match cli.command {
        Command::Query(args) => query::run_query(*args),
        Command::Revert(args) => revert_cmd::run_revert(*args),
        Command::Dream(args) => dream_cmd::run_dream(*args),
        Command::Skills { command } => skills_cmd::run(command),
        Command::Bench { command } => match command {
            BenchCommand::Ltd(args) => toolbench_cmd::run_toolbench(*args),
            BenchCommand::Full(args) => bench_cmd::run_bench(*args),
            BenchCommand::Autonomy(args) => autonomy_cmd::run_autonomy(*args),
        },
        Command::Mcp(args) => mcp::run_mcp(*args),
        Command::Chat(args) => chat::run_chat(*args),
        Command::Launch(args) => launch::run_launch(*args),
        Command::Icm(args) => icm::run_icm(*args),
        Command::Cron(args) => cron::run_cron(*args),
        #[cfg(feature = "backend-openai")]
        Command::Api(args) => {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            rt.block_on(api::server::run_api(*args))
        }
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
