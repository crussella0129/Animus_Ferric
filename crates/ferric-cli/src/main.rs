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
mod human;
mod icm;
mod launch;
#[cfg(all(test, feature = "backend-openai", any(windows, target_os = "linux")))]
mod live_budget_tests;
mod logging;
mod mcp;
mod query;
mod revert_cmd;
mod server;
mod server_process;
mod server_registration;
mod server_resolution;
mod skills_cmd;
#[cfg(feature = "backend-openai")]
mod startup;
mod tailscale_localapi;
mod tailscale_serve;
#[cfg(test)]
mod test_process_containment;
#[cfg(all(test, any(windows, target_os = "linux")))]
mod test_process_containment_tests;
mod toolbench_cmd;
mod trace_cmd;
mod trace_verify;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ferric",
    bin_name = "ferric",
    version,
    about = "A local model, ready to help. Run without arguments to begin.",
    disable_help_subcommand = true
)]
struct Cli {
    /// Increase harness-internal log verbosity (`-v` info, `-vv` debug,
    /// `-vvv` trace). Diagnostics go to stderr — stdout stays a clean machine
    /// channel. `FERRIC_LOG`/`RUST_LOG` override this entirely (ADR-063).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open a session, ask a question, or work on files with permission
    Run(Box<human::RunArgs>),
    /// Describe this folder's setup without probes or changes
    Status(human::DescribeArgs),
    /// Explain settings, resources and intended effects without changes
    Explain(human::DescribeArgs),
    /// Find and use the expert commands
    #[command(disable_help_flag = true)]
    Advanced {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<std::ffi::OsString>,
    },
    /// Run a one-shot, workspace-scoped query against a local model
    #[command(hide = true)]
    Query(Box<query::QueryArgs>),
    /// Run benchmarking suites (ltd for syntax fire-rate, full for L0-L6 ladder)
    #[command(hide = true)]
    Bench {
        #[command(subcommand)]
        command: BenchCommand,
    },
    /// Run the MCP-stdio server (one tool, `ferric_query`; ADR-046)
    #[command(hide = true)]
    Mcp(Box<mcp::McpArgs>),
    /// Start an interactive chat REPL (hybrid talk + `/do` escalate; ADR-052)
    #[command(hide = true)]
    Chat(Box<chat::ChatArgs>),
    /// Bootstrap a new project: scaffold a git repo (main+dev) + skeleton (ADR-053)
    #[command(hide = true)]
    Launch(Box<launch::LaunchArgs>),
    /// ICM agent delegation: scaffold/plan a filesystem-orchestrated workspace (ADR-064)
    #[command(hide = true)]
    Icm(Box<icm::IcmArgs>),
    /// Agentic cron: schedule periodic agent tasks from `.ferric/cron/` (ADR-066)
    #[command(hide = true)]
    Cron(Box<cron::CronArgs>),
    /// Start the HTTP API server for IDE/web/mobile integration (Sprint 64)
    #[cfg(feature = "backend-openai")]
    #[command(hide = true)]
    Api(Box<api::server::ApiArgs>),
    /// Revert the workspace and trace to a specific turn snapshot
    #[command(hide = true)]
    Revert(Box<revert_cmd::RevertArgs>),
    /// Dream Mode: Asynchronously parse historical traces and extract knowledge
    #[command(hide = true)]
    Dream(Box<dream_cmd::DreamArgs>),
    /// Launch and manage the OpenAI-compatible inference server (the HTTP valve)
    #[command(hide = true)]
    Server {
        #[command(subcommand)]
        command: server::ServerCommand,
    },
    /// Inspect installed agent skills
    #[command(hide = true)]
    Skills {
        #[command(subcommand)]
        command: skills_cmd::SkillsCommand,
    },
    /// Inspect session traces
    #[command(hide = true)]
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
    let cli = match resolve_cli(Cli::parse()) {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(code as u8);
        }
    };
    // Install the stderr diagnostic subscriber before any command runs, so
    // every subcommand (and its libraries) emits through one configured sink.
    logging::init(cli.verbose);
    dispatch(cli.command)
}

fn dispatch(command: Option<Command>) -> ExitCode {
    let Some(command) = command else {
        return human::run(human::RunArgs::default());
    };
    match command {
        Command::Run(args) => human::run(*args),
        Command::Status(args) | Command::Explain(args) => human::describe(args),
        Command::Advanced { .. } => advanced_help(),
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

// Resolve the expert doorway before installing logging, so forwarded global
// flags retain their meaning and a missing expert command cannot start a session.
fn resolve_cli(cli: Cli) -> Result<Cli, clap::Error> {
    let Cli {
        verbose,
        command: Some(Command::Advanced { arguments }),
    } = cli
    else {
        return Ok(cli);
    };
    if arguments.is_empty()
        || arguments == [std::ffi::OsString::from("--help")]
        || arguments == [std::ffi::OsString::from("-h")]
    {
        return Ok(Cli {
            verbose,
            command: Some(Command::Advanced { arguments }),
        });
    }
    // One parser owns expert arguments and wire formats; this is a discoverable
    // doorway, not a second command schema that can drift from the original.
    let mut input = vec![std::ffi::OsString::from("ferric")];
    input.extend(arguments);
    let mut resolved = Cli::try_parse_from(input)?;
    if matches!(
        resolved.command,
        None | Some(
            Command::Advanced { .. } | Command::Run(_) | Command::Status(_) | Command::Explain(_)
        )
    ) {
        return Err(Cli::command().error(
            clap::error::ErrorKind::InvalidSubcommand,
            "Choose an expert command after ferric advanced; use ferric run for a human session.",
        ));
    }
    resolved.verbose = verbose.saturating_add(resolved.verbose);
    Ok(resolved)
}

fn advanced_help() -> ExitCode {
    let mut help = Cli::command()
        .name("ferric advanced")
        .about("Expert commands. Their original ferric <command> spellings also work.")
        .mut_subcommands(|command| {
            let primary = matches!(
                command.get_name(),
                "run" | "status" | "explain" | "advanced"
            );
            command.hide(primary)
        });
    if help.print_help().is_ok() {
        println!();
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;

    #[test]
    fn advanced_preserves_both_verbosity_positions() {
        for (args, expected) in [
            (
                vec!["ferric", "advanced", "query", "--mock", "-vv", "hello"],
                2,
            ),
            (
                vec![
                    "ferric", "-v", "advanced", "query", "--mock", "-vv", "hello",
                ],
                3,
            ),
        ] {
            let cli = resolve_cli(Cli::try_parse_from(args).unwrap()).unwrap();
            assert_eq!(cli.verbose, expected);
            assert!(matches!(cli.command, Some(Command::Query(_))));
        }
    }

    #[test]
    fn advanced_flags_alone_never_enter_human_session() {
        let cli = Cli::try_parse_from(["ferric", "advanced", "--", "-v"]).unwrap();
        assert!(resolve_cli(cli).is_err());
    }
}
