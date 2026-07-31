//! The benchmark runner: materialize a workspace, spawn the `ferric` binary
//! as a `query` subprocess (spawn-self by default — the child is always
//! `query`, so `bench` recursion is structurally impossible), enforce the
//! spec's wall-clock timeout, and locate the resulting trace.
//!
//! Std-only (no tokio in the default graph): the timeout is a `try_wait`
//! poll loop. Release-profile children are required for usable speed (debug
//! candle is ~1 tok/s — s1 lesson); the CLI warns under debug_assertions.

use std::path::{Path, PathBuf};
use std::process::Command;
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

/// Materialize the spec's workspace, run the agent, and return the raw record.
pub fn run_spec(spec: &BenchSpec, inv: &Invocation) -> std::io::Result<RunRecord> {
    let dir = tempfile::tempdir()?;
    for (rel, content) in &spec.setup_files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }

    let mut cmd = Command::new(&inv.ferric_bin);
    cmd.args(query_args(&spec.prompt, inv, dir.path()));
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let started = Instant::now();
    let mut child = cmd.spawn()?;
    let timeout = Duration::from_secs(spec.timeout_s);
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

    // Drain stderr tail (the child closes its pipes on exit/kill).
    let stderr_tail = match child.wait_with_output() {
        Ok(out) => tail(&String::from_utf8_lossy(&out.stderr), 1000),
        Err(_) => String::new(),
    };

    let trace_path = find_trace(dir.path());

    let workspace = if inv.keep_workspace {
        let kept = dir.keep();
        WorkspaceHandle::Kept(kept)
    } else {
        WorkspaceHandle::Temp(dir)
    };

    Ok(RunRecord {
        exit_code,
        timed_out,
        wall,
        trace_path,
        workspace,
        stderr_tail,
    })
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
fn query_args(prompt: &str, inv: &Invocation, workspace: &Path) -> Vec<String> {
    let mut args = vec![
        "query".to_string(),
        prompt.to_string(),
        "--workspace".to_string(),
        workspace.display().to_string(),
        "--protocol".to_string(),
        protocol_flag(inv.protocol).to_string(),
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

fn tail(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    s.chars()
        .rev()
        .take(n)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
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
        let args = query_args("do a task", &inv, Path::new("/ws"));
        assert!(has_pair(&args, "--model", "qwen2.5-coder:7b"));
        assert!(has_pair(&args, "--api-base", "http://localhost:11434/v1"));
        assert!(has_pair(&args, "--protocol", "grammar"));
        // The single-backend simplification removed `--backend`; the child
        // `query` no longer accepts it, so the runner must never emit it.
        assert!(!args.iter().any(|a| a == "--backend"), "{args:?}");
        assert!(!args.iter().any(|a| a == "--mock"));
    }

    #[test]
    fn query_args_mock_arm() {
        let args = query_args("t", &base(), Path::new("/ws"));
        assert!(args.iter().any(|a| a == "--mock"));
        assert!(!args.iter().any(|a| a == "--backend"));
    }
}
