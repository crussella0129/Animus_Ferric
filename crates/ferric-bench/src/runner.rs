//! The benchmark runner: materialize a workspace, spawn the `ferric` binary
//! as a `query` subprocess (spawn-self by default — the child is always
//! `query`, so `bench` recursion is structurally impossible), enforce the
//! spec's wall-clock timeout, and locate the resulting trace.
//!
//! Std-only (no tokio in the default graph): the timeout is a `try_wait`
//! poll loop. Release-profile children are required for usable speed (debug
//! candle is ~1 tok/s — s1 lesson); the CLI warns under debug_assertions.

use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ferric_core::{ActionProtocol, HarnessPolicy};
use tempfile::TempDir;

use crate::spec::BenchSpec;

/// How to invoke the agent for a benchmark run.
#[derive(Clone)]
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

#[derive(Clone)]
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

/// One process segment in the autonomy matrix. Initial segments supply a
/// prompt; continuation segments supply a prior trace and, when applicable, a
/// clarification answer. The workspace and isolated profile directory survive
/// across every segment in an episode.
pub struct QuerySegmentRequest<'a> {
    pub workspace: &'a Path,
    pub profile_dir: &'a Path,
    pub checks_file: Option<&'a Path>,
    pub prompt: Option<&'a str>,
    pub resume: Option<&'a Path>,
    pub answer: Option<&'a str>,
    pub max_turns: u32,
    pub timeout: Duration,
    /// Override only this segment's OpenAI endpoint. The autonomy provider-
    /// failure fixture uses an unreachable loopback endpoint, then resumes
    /// against the ordinary endpoint in the next process.
    pub api_base_override: Option<&'a str>,
    /// Optional autonomous-controller flag for binaries which support it.
    /// `None` is intentional: frozen legacy controls predate this flag and
    /// must continue to receive the historical child argv unchanged.
    pub harness_policy: Option<HarnessPolicy>,
}

/// Raw process evidence for one autonomy segment. A rejected resume normally
/// has no new trace; that is data for a refusal probe rather than a discovery
/// failure.
pub struct QuerySegmentRecord {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub wall: Duration,
    pub trace_path: Option<PathBuf>,
    pub trace_discovery_error: Option<String>,
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
    if inv.prompts_dir.is_none() {
        cmd.env_remove("FERRIC_PROMPTS_DIR");
    }

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

/// Spawn one isolated `ferric query` process without creating or deleting the
/// workspace. This is the process boundary used by the autonomy runner to
/// exercise real pause/resume and resume-of-resume chains.
pub fn run_query_segment(
    inv: &Invocation,
    request: &QuerySegmentRequest<'_>,
) -> std::io::Result<QuerySegmentRecord> {
    validate_segment_request(request)?;
    let before = trace_files(request.workspace)?;
    let mut cmd = Command::new(&inv.ferric_bin);
    cmd.args(query_segment_args(inv, request));
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if inv.prompts_dir.is_none() {
        cmd.env_remove("FERRIC_PROMPTS_DIR");
    }

    let started = Instant::now();
    let child = cmd.spawn()?;
    let outcome = wait_for_child(child, started, request.timeout)?;
    let after = trace_files(request.workspace)?;
    let created: Vec<PathBuf> = after.difference(&before).cloned().collect();
    let (trace_path, trace_discovery_error) = match created.as_slice() {
        [] => (None, None),
        [path] => (Some(path.clone()), None),
        paths => (
            None,
            Some(format!(
                "query segment created {} traces; expected exactly one",
                paths.len()
            )),
        ),
    };

    Ok(QuerySegmentRecord {
        exit_code: outcome.exit_code,
        timed_out: outcome.timed_out,
        wall: outcome.wall,
        trace_path,
        trace_discovery_error,
        stderr_tail: outcome.stderr_tail,
    })
}

fn validate_segment_request(request: &QuerySegmentRequest<'_>) -> io::Result<()> {
    match (request.prompt, request.resume) {
        (Some(prompt), None) if !prompt.trim().is_empty() => {}
        (None, Some(_)) => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "query segment requires exactly one of a non-empty prompt or resume trace",
            ));
        }
    }
    if request.answer.is_some() && request.resume.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "query segment answer requires a resume trace",
        ));
    }
    if request.max_turns == 0 || request.max_turns > u32::from(u8::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "query segment max_turns must be in 1..=255",
        ));
    }
    Ok(())
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
        "0.0".to_string(),
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

fn query_segment_args(inv: &Invocation, request: &QuerySegmentRequest<'_>) -> Vec<String> {
    let mut args = vec!["query".to_string()];
    if let Some(prompt) = request.prompt {
        args.push(prompt.to_string());
    } else if let Some(resume) = request.resume {
        args.extend(["--resume".to_string(), resume.display().to_string()]);
    }
    if let Some(answer) = request.answer {
        args.extend(["--answer".to_string(), answer.to_string()]);
    }
    args.extend([
        "--workspace".to_string(),
        request.workspace.display().to_string(),
        "--protocol".to_string(),
        protocol_flag(inv.protocol).to_string(),
        "--no-config".to_string(),
        "--profile-dir".to_string(),
        request.profile_dir.display().to_string(),
        "--temperature".to_string(),
        "0.0".to_string(),
        "--max-turns".to_string(),
        request.max_turns.to_string(),
        "--no-stream".to_string(),
    ]);
    if let Some(checks) = request.checks_file {
        args.extend(["--checks-file".to_string(), checks.display().to_string()]);
    }
    if let Some(openai) = &inv.openai {
        if let Some(base) = request.api_base_override.or(openai.api_base.as_deref()) {
            args.extend(["--api-base".to_string(), base.to_string()]);
        }
        args.extend([
            "--model".to_string(),
            openai.model.clone(),
            "--params-b".to_string(),
            openai.params_b.to_string(),
            "--ctx".to_string(),
            openai.ctx.to_string(),
        ]);
    } else {
        args.push("--mock".to_string());
    }
    if request.resume.is_none()
        && let Some(prompts) = &inv.prompts_dir
    {
        args.extend(["--prompts-dir".to_string(), prompts.display().to_string()]);
    }
    if let Some(policy) = request.harness_policy {
        args.extend(["--harness-policy".to_string(), policy.to_string()]);
    }
    args
}

fn trace_files(workspace: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let trace_dir = ferric_trace::trace_dir(workspace);
    let entries = match std::fs::read_dir(trace_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error),
    };
    let mut traces = BTreeSet::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("q-") && name.ends_with(".jsonl"))
        {
            traces.insert(path);
        }
    }
    Ok(traces)
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
        assert!(has_pair(&args, "--temperature", "0.0"));
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
    fn autonomy_resume_args_preserve_isolation_and_answer() {
        let mut inv = base();
        inv.openai = Some(OpenAiArgs {
            api_base: Some("http://127.0.0.1:8080/v1".to_string()),
            model: "model.gguf".to_string(),
            params_b: 7.6,
            ctx: 8192,
        });
        let mut request = QuerySegmentRequest {
            workspace: Path::new("/ws"),
            profile_dir: Path::new("/profiles"),
            checks_file: Some(Path::new("/checks.toml")),
            prompt: None,
            resume: Some(Path::new("/ws/.ferric/trace/prior.jsonl")),
            answer: Some("use UTC"),
            max_turns: 9,
            timeout: Duration::from_secs(60),
            api_base_override: Some("http://127.0.0.1:9/v1"),
            harness_policy: None,
        };
        let control_args = query_segment_args(&inv, &request);
        request.harness_policy = Some(HarnessPolicy::Legacy);
        let explicit_legacy_args = query_segment_args(&inv, &request);
        let mut expected_explicit_legacy = control_args.clone();
        expected_explicit_legacy.extend(["--harness-policy".to_string(), "legacy".to_string()]);
        assert_eq!(explicit_legacy_args, expected_explicit_legacy);
        request.harness_policy = Some(HarnessPolicy::Evidence);
        let args = query_segment_args(&inv, &request);
        let mut expected_candidate = control_args;
        expected_candidate.extend(["--harness-policy".to_string(), "evidence".to_string()]);
        assert_eq!(args, expected_candidate);
        assert!(has_pair(&args, "--resume", "/ws/.ferric/trace/prior.jsonl"));
        assert!(has_pair(&args, "--answer", "use UTC"));
        assert!(has_pair(&args, "--checks-file", "/checks.toml"));
        assert!(has_pair(&args, "--api-base", "http://127.0.0.1:9/v1"));
        assert!(has_pair(&args, "--temperature", "0.0"));
        assert!(has_pair(&args, "--harness-policy", "evidence"));
        assert!(args.iter().any(|arg| arg == "--no-config"));
        assert!(args.iter().any(|arg| arg == "--no-stream"));
        assert!(!args.iter().any(|arg| arg == "--prompts-dir"));
    }

    #[test]
    fn segment_request_rejects_mixed_or_missing_sources() {
        let request = QuerySegmentRequest {
            workspace: Path::new("/ws"),
            profile_dir: Path::new("/profiles"),
            checks_file: None,
            prompt: Some("new task"),
            resume: Some(Path::new("trace.jsonl")),
            answer: None,
            max_turns: 1,
            timeout: Duration::from_secs(1),
            api_base_override: None,
            harness_policy: None,
        };
        assert_eq!(
            validate_segment_request(&request).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn trace_discovery_ignores_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let trace_dir = ferric_trace::trace_dir(dir.path());
        std::fs::create_dir_all(&trace_dir).unwrap();
        std::fs::write(trace_dir.join("q-one.jsonl"), "").unwrap();
        std::fs::write(trace_dir.join("mcp-two.jsonl"), "").unwrap();
        std::fs::write(trace_dir.join("q-note.txt"), "").unwrap();
        let traces = trace_files(dir.path()).unwrap();
        assert_eq!(traces, BTreeSet::from([trace_dir.join("q-one.jsonl")]));
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
