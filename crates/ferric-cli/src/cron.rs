//! `ferric cron` — agentic cron (ADR-066, sprint 75).
//!
//! Schedules periodic *agent* tasks defined in `<workspace>/.ferric/cron/*.toml`.
//! Each job names a schedule (`12h`, `daily`, …) and a command — one of Ferric's
//! own guard-contained operations (`dream`, or a `query` with a prompt). The
//! watcher checks which jobs are due and runs them by shelling out to this same
//! `ferric` binary, so a scheduled task is always a contained Ferric subcommand,
//! never an arbitrary shell command.
//!
//! - `ferric cron add <name> --schedule 12h --command dream`
//! - `ferric cron list`                 — jobs, schedules, last-run / next-due
//! - `ferric cron run [--dry-run]`       — run all currently-due jobs once
//! - `ferric cron watch [--interval 60s]`— loop, running due jobs until Ctrl-C
//!
//! Pure scheduling/parsing lives in `ferric-cron`; this module drives it and
//! performs execution. Last-run state is `.ferric/cron/.state.json`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use clap::{Args, Subcommand};
use ferric_cron::{
    CronState, JobCommand, due_jobs, job_toml, load_jobs, next_due_ms, parse_interval_ms,
    parse_schedule,
};

use crate::query::now_ms;

#[derive(Args)]
pub struct CronArgs {
    /// Workspace root (holds `.ferric/cron/`). Default: current directory.
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,

    #[command(subcommand)]
    command: CronCommand,
}

#[derive(Subcommand)]
enum CronCommand {
    /// Create a job file under `.ferric/cron/<name>.toml`
    Add {
        /// Job name (the file stem)
        name: String,
        /// Schedule interval: `30s`, `15m`, `12h`, `2d`, or `hourly`/`daily`/`weekly`
        #[arg(long)]
        schedule: String,
        /// What to run: `dream` or `query`
        #[arg(long)]
        command: String,
        /// The prompt (required for `--command query`)
        #[arg(long)]
        prompt: Option<String>,
        /// For a query job: run against the offline mock (for testing schedules)
        #[arg(long)]
        mock: bool,
    },
    /// List configured jobs with their schedule, last-run and next-due
    List,
    /// Run every currently-due job once, then exit
    Run {
        /// Report which jobs are due without running them or touching state
        #[arg(long)]
        dry_run: bool,
    },
    /// Watch: run due jobs on every tick until interrupted (Ctrl-C)
    Watch {
        /// How often to check for due jobs (default `60s`)
        #[arg(long, default_value = "60s")]
        interval: String,
    },
}

pub fn run_cron(args: CronArgs) -> ExitCode {
    let workspace = args
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let cron_dir = workspace.join(".ferric").join("cron");
    let state_path = cron_dir.join(".state.json");

    match args.command {
        CronCommand::Add {
            name,
            schedule,
            command,
            prompt,
            mock,
        } => run_add(&cron_dir, &name, &schedule, &command, prompt, mock),
        CronCommand::List => run_list(&cron_dir, &state_path),
        CronCommand::Run { dry_run } => {
            match run_due_once(&workspace, &cron_dir, &state_path, dry_run) {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("cron run failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        CronCommand::Watch { interval } => run_watch(&workspace, &cron_dir, &state_path, &interval),
    }
}

fn run_add(
    cron_dir: &Path,
    name: &str,
    schedule: &str,
    command: &str,
    prompt: Option<String>,
    mock: bool,
) -> ExitCode {
    // Validate the schedule up front so a bad job file is never written.
    if let Err(e) = parse_schedule(schedule) {
        eprintln!("cron add: {e}");
        return ExitCode::FAILURE;
    }
    let job_command = match command.trim().to_ascii_lowercase().as_str() {
        "dream" => JobCommand::Dream,
        "query" => match prompt {
            Some(p) if !p.trim().is_empty() => JobCommand::Query { prompt: p, mock },
            _ => {
                eprintln!("cron add: --command query requires a non-empty --prompt");
                return ExitCode::FAILURE;
            }
        },
        other => {
            eprintln!("cron add: unknown command '{other}' (expected `dream` or `query`)");
            return ExitCode::FAILURE;
        }
    };

    let path = cron_dir.join(format!("{name}.toml"));
    if path.exists() {
        eprintln!(
            "cron add: {} already exists (edit it directly)",
            path.display()
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::create_dir_all(cron_dir) {
        eprintln!("cron add: cannot create {}: {e}", cron_dir.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(&path, job_toml(schedule, &job_command)) {
        eprintln!("cron add: cannot write {}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!(
        "Added cron job '{name}' ({schedule}, {}) at {}",
        job_command.kind(),
        path.display()
    );
    ExitCode::SUCCESS
}

fn run_list(cron_dir: &Path, state_path: &Path) -> ExitCode {
    let jobs = match load_jobs(cron_dir) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cron list failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    if jobs.is_empty() {
        println!("No cron jobs configured in {}", cron_dir.display());
        return ExitCode::SUCCESS;
    }
    let state = CronState::load(state_path);
    let now = now_ms() as u64;

    println!("{} cron job(s) in {}:", jobs.len(), cron_dir.display());
    for job in &jobs {
        let last = state.last_run(&job.name);
        let last_str = match last {
            Some(t) => format!("{} ago", human_ms(now.saturating_sub(t))),
            None => "never".to_string(),
        };
        let next_str = match next_due_ms(job, last, now) {
            _ if !job.enabled => "disabled".to_string(),
            None => "now".to_string(),
            Some(t) if t <= now => "now".to_string(),
            Some(t) => format!("in {}", human_ms(t.saturating_sub(now))),
        };
        println!(
            "  {:<16} {:<18} {:<7} last: {:<14} next: {}",
            job.name,
            job.schedule.describe(),
            job.command.kind(),
            last_str,
            next_str
        );
    }
    ExitCode::SUCCESS
}

/// Run every due job once. Returns the number of jobs run. State is advanced per
/// job on ATTEMPT (success or failure), so a persistently-failing job reschedules
/// to its next interval instead of firing every tick.
fn run_due_once(
    workspace: &Path,
    cron_dir: &Path,
    state_path: &Path,
    dry_run: bool,
) -> Result<usize, String> {
    let jobs = load_jobs(cron_dir).map_err(|e| e.to_string())?;
    let mut state = CronState::load(state_path);
    let now = now_ms() as u64;
    let due = due_jobs(&jobs, &state, now);

    if due.is_empty() {
        println!("No jobs due.");
        return Ok(0);
    }

    let mut ran = 0;
    for job in due {
        if dry_run {
            println!("DUE (dry-run): {} [{}]", job.name, job.command.kind());
            continue;
        }
        println!(
            "▶ Running cron job '{}' [{}]…",
            job.name,
            job.command.kind()
        );
        match spawn_job(workspace, &job.command) {
            Ok(true) => println!("✔ '{}' completed.", job.name),
            Ok(false) => eprintln!("✖ '{}' exited non-zero.", job.name),
            Err(e) => eprintln!("✖ '{}' could not start: {e}", job.name),
        }
        // Advance state regardless of outcome so the interval is respected.
        state.mark_run(&job.name, now);
        ran += 1;
    }
    if ran > 0 {
        state.save(state_path).map_err(|e| e.to_string())?;
    }
    Ok(ran)
}

fn run_watch(workspace: &Path, cron_dir: &Path, state_path: &Path, interval: &str) -> ExitCode {
    let interval = match parse_interval_ms(interval) {
        Ok(ms) => Duration::from_millis(ms),
        Err(e) => {
            eprintln!("cron watch: {e}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cron watch: tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "Watching {} every {} (Ctrl-C to stop)…",
        cron_dir.display(),
        interval_str(interval)
    );
    runtime.block_on(async {
        loop {
            if let Err(e) = run_due_once(workspace, cron_dir, state_path, false) {
                eprintln!("cron tick failed: {e}");
            }
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    println!("\nStopping cron watcher.");
                    break;
                }
                _ = tokio::time::sleep(interval) => {}
            }
        }
    });
    ExitCode::SUCCESS
}

/// Spawn one job as a `ferric` subprocess in `workspace`, blocking until it
/// exits. Returns whether it exited successfully. The command is one of Ferric's
/// own subcommands — never an arbitrary shell string.
fn spawn_job(workspace: &Path, command: &JobCommand) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| format!("locating ferric: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.current_dir(workspace);
    match command {
        JobCommand::Dream => {
            cmd.arg("dream");
        }
        JobCommand::Query { prompt, mock } => {
            cmd.arg("query").arg("--workspace").arg(workspace);
            if *mock {
                cmd.arg("--mock");
            }
            // `--` guards against a prompt that begins with `-`.
            cmd.arg("--").arg(prompt);
        }
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    Ok(status.success())
}

fn interval_str(d: Duration) -> String {
    human_ms(d.as_millis() as u64)
}

/// Compact human duration for `list`/`watch` output.
fn human_ms(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3_600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3_600)
    } else {
        format!("{}d", s / 86_400)
    }
}
