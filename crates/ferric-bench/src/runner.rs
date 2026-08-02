//! The benchmark runner: materialize a workspace, spawn the `ferric` binary
//! as a `query` subprocess (spawn-self by default — the child is always
//! `query`, so `bench` recursion is structurally impossible), enforce the
//! spec's wall-clock timeout, and locate the resulting trace.
//!
//! Std-only (no tokio in the default graph): the timeout is a `try_wait`
//! poll loop. Release-profile children are required for usable speed (debug
//! candle is ~1 tok/s — s1 lesson); the CLI warns under debug_assertions.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ferric_core::ActionProtocol;
use tempfile::TempDir;

use crate::spec::BenchSpec;

/// How to invoke the agent for a benchmark run.
pub struct Invocation {
    /// The `ferric` binary to spawn (default: this executable).
    pub ferric_bin: PathBuf,
    pub protocol: ActionProtocol,
    /// OpenAI-compatible backend (ollama / llama-server — the constrained
    /// workhorse), or `None` for `--mock`. The only real backend since the
    /// in-process mistral.rs GGUF path was removed (ADR-027); `requires
    /// backend-openai` in the spawned binary.
    pub openai: Option<OpenAiArgs>,
    /// Prompt library dir passed through as `--prompts-dir`.
    pub prompts_dir: Option<PathBuf>,
    pub keep_workspace: bool,
}

pub struct OpenAiArgs {
    /// `--api-base` (e.g. `http://localhost:11434/v1`); `None` auto-discovers a
    /// `ferric server` runfile, like `query` itself.
    pub api_base: Option<String>,
    /// The model identifier (`--model`, e.g. `qwen2.5-coder:7b`).
    pub model: String,
    pub params_b: f32,
    pub ctx: u32,
}

impl Invocation {
    pub fn mock() -> Self {
        Self {
            ferric_bin: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ferric")),
            protocol: ActionProtocol::ConstrainedJson,
            openai: None,
            prompts_dir: None,
            keep_workspace: false,
        }
    }
}

/// The raw outcome of one spawned run (verification happens in `verify`).
pub struct RunRecord {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub wall: Duration,
    /// The single `q-*.jsonl` trace the child wrote, if found.
    pub trace_path: Option<PathBuf>,
    /// Kept alive so the workspace survives until the caller is done; `None`
    /// when `keep_workspace` moved it to a persisted path.
    pub workspace: WorkspaceHandle,
    pub stderr_tail: String,
}

pub enum WorkspaceHandle {
    Temp(TempDir),
    Kept(PathBuf),
}

impl WorkspaceHandle {
    pub fn path(&self) -> &Path {
        match self {
            WorkspaceHandle::Temp(d) => d.path(),
            WorkspaceHandle::Kept(p) => p.as_path(),
        }
    }
}

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const STDERR_TAIL_BYTES: usize = 1000;

struct ChildOutcome {
    exit_code: Option<i32>,
    timed_out: bool,
    wall: Duration,
    stderr_tail: String,
}

/// Materialize the spec's workspace, run the agent, and return the raw record.
pub fn run_spec(spec: &BenchSpec, inv: &Invocation) -> std::io::Result<RunRecord> {
    let dir = tempfile::tempdir()?;
    // A calibration run must not consume a profile written by an earlier run.
    // Keep the empty profile directory alive until the child exits.
    let profile_dir = tempfile::tempdir()?;
    for (rel, content) in &spec.setup_files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }

    let mut cmd = Command::new(&inv.ferric_bin);
    cmd.args(query_args(
        &spec.prompt,
        spec.max_turns,
        inv,
        dir.path(),
        profile_dir.path(),
    ));
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let started = Instant::now();
    let child = cmd.spawn()?;
    let outcome = wait_for_child(child, started, Duration::from_secs(spec.timeout_s))?;

    let trace_path = find_trace(dir.path());

    let workspace = if inv.keep_workspace {
        let kept = dir.keep();
        WorkspaceHandle::Kept(kept)
    } else {
        WorkspaceHandle::Temp(dir)
    };

    Ok(RunRecord {
        exit_code: outcome.exit_code,
        timed_out: outcome.timed_out,
        wall: outcome.wall,
        trace_path,
        workspace,
        stderr_tail: outcome.stderr_tail,
    })
}

/// Drain both child pipes while polling. Waiting to read until after exit can
/// deadlock when a verbose child fills an OS pipe and blocks before exiting.
fn wait_for_child(
    mut child: Child,
    started: Instant,
    timeout: Duration,
) -> io::Result<ChildOutcome> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("child stdout was not piped"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("child stderr was not piped"))?;

    let stdout_drain = std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut sink = io::sink();
        io::copy(&mut stdout, &mut sink).map(|_| ())
    });
    let stderr_drain = std::thread::spawn(move || read_tail(stderr, STDERR_TAIL_BYTES));

    let mut timed_out = false;
    let exit_code = loop {
        match child.try_wait()? {
            Some(status) => break status.code(),
            None => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    };
    let wall = started.elapsed();

    join_drain(stdout_drain, "stdout")?;
    let stderr = join_drain(stderr_drain, "stderr")?;

    Ok(ChildOutcome {
        exit_code,
        timed_out,
        wall,
        stderr_tail: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

fn join_drain<T>(handle: JoinHandle<io::Result<T>>, pipe: &str) -> io::Result<T> {
    handle
        .join()
        .map_err(|_| io::Error::other(format!("{pipe} drain thread panicked")))?
}

fn read_tail(mut reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut tail = Vec::with_capacity(limit);
    let mut buf = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            return Ok(tail);
        }
        if limit == 0 {
            continue;
        }
        if read >= limit {
            tail.clear();
            tail.extend_from_slice(&buf[read - limit..read]);
            continue;
        }
        let overflow = tail.len().saturating_add(read).saturating_sub(limit);
        if overflow > 0 {
            tail.drain(..overflow);
        }
        tail.extend_from_slice(&buf[..read]);
    }
}

fn protocol_flag(p: ActionProtocol) -> &'static str {
    match p {
        ActionProtocol::NativeTools => "native",
        ActionProtocol::ConstrainedJson => "grammar",
        ActionProtocol::TextXml => "xml",
        ActionProtocol::Plan => "plan",
    }
}

/// Build the child `query` argv (excluding the binary). Pure, so the backend
/// branching is unit-testable without spawning. Either the `openai` backend
/// (the constrained workhorse — ollama/llama-server) or `--mock`; the
/// in-process mistral.rs GGUF path was removed (ADR-027), so the openai arm is
/// how the full loop reaches a real *constrained* model.
fn query_args(
    prompt: &str,
    max_turns: u32,
    inv: &Invocation,
    workspace: &Path,
    profile_dir: &Path,
) -> Vec<String> {
    let mut args = vec![
        "query".to_string(),
        prompt.to_string(),
        "--workspace".to_string(),
        workspace.display().to_string(),
        "--protocol".to_string(),
        protocol_flag(inv.protocol).to_string(),
        "--no-config".to_string(),
        "--profile-dir".to_string(),
        profile_dir.display().to_string(),
        "--temperature".to_string(),
        "0".to_string(),
        "--max-turns".to_string(),
        max_turns.to_string(),
        "--no-stream".to_string(),
    ];
    if let Some(o) = &inv.openai {
        if let Some(base) = &o.api_base {
            args.push("--api-base".to_string());
            args.push(base.clone());
        }
        args.extend([
            "--model".to_string(),
            o.model.clone(),
            "--params-b".to_string(),
            o.params_b.to_string(),
            "--ctx".to_string(),
            o.ctx.to_string(),
        ]);
    } else {
        args.push("--mock".to_string());
    }
    if let Some(prompts) = &inv.prompts_dir {
        args.push("--prompts-dir".to_string());
        args.push(prompts.display().to_string());
    }
    args
}

/// Find the single `q-*.jsonl` the child wrote under `<ws>/.ferric/trace/`.
fn find_trace(workspace: &Path) -> Option<PathBuf> {
    let trace_dir = ferric_trace::trace_dir(workspace);
    std::fs::read_dir(&trace_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("q-") && n.ends_with(".jsonl"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Invocation {
        Invocation {
            ferric_bin: PathBuf::from("ferric"),
            protocol: ActionProtocol::ConstrainedJson,
            openai: None,
            prompts_dir: None,
            keep_workspace: false,
        }
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn query_args_openai_arm_targets_the_valve() {
        let mut inv = base();
        inv.openai = Some(OpenAiArgs {
            api_base: Some("http://localhost:11434/v1".to_string()),
            model: "qwen2.5-coder:7b".to_string(),
            params_b: 7.0,
            ctx: 4096,
        });
        let args = query_args(
            "do a task",
            12,
            &inv,
            Path::new("/ws"),
            Path::new("/profiles"),
        );
        assert!(has_pair(&args, "--model", "qwen2.5-coder:7b"));
        assert!(has_pair(&args, "--api-base", "http://localhost:11434/v1"));
        assert!(has_pair(&args, "--protocol", "grammar"));
        assert!(has_pair(&args, "--profile-dir", "/profiles"));
        assert!(has_pair(&args, "--temperature", "0"));
        assert!(has_pair(&args, "--max-turns", "12"));
        assert!(args.iter().any(|a| a == "--no-config"));
        assert!(args.iter().any(|a| a == "--no-stream"));
        // The single-backend simplification removed `--backend`; the child
        // `query` no longer accepts it, so the runner must never emit it.
        assert!(!args.iter().any(|a| a == "--backend"), "{args:?}");
        assert!(!args.iter().any(|a| a == "--mock"));
    }

    #[test]
    fn query_args_mock_arm() {
        let args = query_args("t", 5, &base(), Path::new("/ws"), Path::new("/profiles"));
        assert!(args.iter().any(|a| a == "--mock"));
        assert!(!args.iter().any(|a| a == "--backend"));
    }

    #[test]
    fn read_tail_is_bounded_and_keeps_the_end() {
        let input = format!("{}THE-END", "x".repeat(128 * 1024));
        let tail = read_tail(std::io::Cursor::new(input), 1000).unwrap();
        assert_eq!(tail.len(), 1000);
        assert!(tail.ends_with(b"THE-END"));
    }

    #[test]
    fn verbose_child_cannot_fill_pipes_and_deadlock() {
        let mut cmd = noisy_child_command();
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let started = Instant::now();
        let child = cmd.spawn().expect("spawn noisy child");
        let outcome = wait_for_child(child, started, Duration::from_secs(15)).unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        assert!(!outcome.timed_out);
        assert!(outcome.stderr_tail.ends_with("THE-END"));
        assert!(outcome.stderr_tail.len() <= STDERR_TAIL_BYTES);
    }

    #[cfg(windows)]
    fn noisy_child_command() -> Command {
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$s = ('x' * 131072) -join ''; [Console]::Out.Write($s); [Console]::Error.Write($s + 'THE-END')",
        ]);
        cmd
    }

    #[cfg(not(windows))]
    fn noisy_child_command() -> Command {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "head -c 131072 /dev/zero; head -c 131072 /dev/zero >&2; printf THE-END >&2",
        ]);
        cmd
    }
}
