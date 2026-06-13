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
    /// `None` → `--mock`; `Some` → real backend (requires the feature build).
    pub model: Option<ModelArgs>,
    /// Prompt library dir passed through as `--prompts-dir`.
    pub prompts_dir: Option<PathBuf>,
    pub keep_workspace: bool,
}

pub struct ModelArgs {
    pub model_dir: PathBuf,
    pub model_file: String,
    pub params_b: f32,
    pub ctx: u32,
}

impl Invocation {
    pub fn mock() -> Self {
        Self {
            ferric_bin: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ferric")),
            protocol: ActionProtocol::UnifiedGrammar,
            model: None,
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
    cmd.arg("query")
        .arg(&spec.prompt)
        .arg("--workspace")
        .arg(dir.path())
        .arg("--protocol")
        .arg(protocol_flag(inv.protocol));
    if let Some(model) = &inv.model {
        cmd.arg("--model-dir")
            .arg(&model.model_dir)
            .arg("--model-file")
            .arg(&model.model_file)
            .arg("--params-b")
            .arg(model.params_b.to_string())
            .arg("--ctx")
            .arg(model.ctx.to_string());
    } else {
        cmd.arg("--mock");
    }
    if let Some(prompts) = &inv.prompts_dir {
        cmd.arg("--prompts-dir").arg(prompts);
    }
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
        ActionProtocol::UnifiedGrammar => "grammar",
    }
}

/// Find the single `q-*.jsonl` the child wrote under `<ws>/.ferric/trace/`.
fn find_trace(workspace: &Path) -> Option<PathBuf> {
    let trace_dir = workspace.join(".ferric").join("trace");
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
