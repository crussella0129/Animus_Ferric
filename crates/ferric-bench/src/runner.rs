//! The benchmark runner: materialize a workspace, spawn the `ferric` binary
//! as a `query` subprocess (spawn-self by default — the child is always
//! `query`, so `bench` recursion is structurally impossible), enforce the
//! spec's wall-clock timeout, and locate the resulting trace.
//!
//! Std-only (no tokio in the default graph): the timeout is a `try_wait`
//! poll loop. External inference speed belongs to the selected server/runtime,
//! not the build profile of this HTTP client.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use ferric_core::{ActionProtocol, HarnessPolicy};
use tempfile::TempDir;

use crate::budget::{AttemptBudgetEvidence, BudgetControls, ResolvedAgentBudget};
use crate::process::{CapturePlan, run_bounded};
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
    /// None preserves historical initial/continuation argv. Selected controls
    /// propagate declared parameters/context to both real and mock children.
    pub budget: Option<BudgetControls>,
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
            budget: None,
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
    /// Known parent enforcement even when the child creates no trace. Legacy
    /// callers without selected controls retain unknown attribution.
    pub budget: Option<AttemptBudgetEvidence>,
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

const STDERR_TAIL_BYTES: usize = 1000;

/// Materialize the spec's workspace, run the agent, and return the raw record.
pub fn run_spec(spec: &BenchSpec, inv: &Invocation) -> std::io::Result<RunRecord> {
    match &inv.budget {
        Some(controls) => {
            let budget = controls
                .resolve_agent(spec.timeout_s)
                .map_err(invalid_budget)?;
            run_spec_with_budget(spec, inv, &budget)
        }
        None => run_spec_inner(spec, inv, Duration::from_secs(spec.timeout_s), None),
    }
}

/// Execute one pre-resolved selected spec budget. Validation is before even
/// temporary workspace/profile allocation; no silent re-resolution per trial.
pub fn run_spec_with_budget(
    spec: &BenchSpec,
    inv: &Invocation,
    budget: &ResolvedAgentBudget,
) -> io::Result<RunRecord> {
    validate_invocation_budget(inv)?;
    let controls = budget.controls();
    if budget.base_timeout_s() != spec.timeout_s
        || match &inv.budget {
            Some(selected) => selected != controls,
            None => controls.timeout_scale() != 1.0 || controls.max_output_tokens().is_some(),
        }
    {
        return Err(invalid_budget(
            "resolved budget does not match selected spec/invocation",
        ));
    }
    if let Some(openai) = &inv.openai
        && (openai.params_b != controls.params_b() || openai.ctx != controls.ctx())
    {
        return Err(invalid_budget(
            "resolved budget does not match real child parameters/context",
        ));
    }
    run_spec_inner(spec, inv, budget.duration(), Some(budget))
}

fn invalid_budget(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn validate_invocation_budget(inv: &Invocation) -> io::Result<()> {
    if let (Some(controls), Some(openai)) = (&inv.budget, &inv.openai)
        && (openai.params_b != controls.params_b() || openai.ctx != controls.ctx())
    {
        return Err(invalid_budget(
            "selected budget does not match real child parameters/context",
        ));
    }
    Ok(())
}

fn run_spec_inner(
    spec: &BenchSpec,
    inv: &Invocation,
    timeout: Duration,
    budget: Option<&ResolvedAgentBudget>,
) -> io::Result<RunRecord> {
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
    cmd.stdin(std::process::Stdio::null());
    if inv.prompts_dir.is_none() {
        cmd.env_remove("FERRIC_PROMPTS_DIR");
    }

    let outcome = run_bounded(
        &mut cmd,
        timeout,
        CapturePlan::stderr_tail(STDERR_TAIL_BYTES),
    )?;

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
        stderr_tail: String::from_utf8_lossy(&outcome.stderr).into_owned(),
        budget: budget.map(|budget| budget.evidence(outcome.exit_code, outcome.timed_out)),
    })
}

/// Spawn one isolated `ferric query` process without creating or deleting the
/// workspace. This is the process boundary used by the autonomy runner to
/// exercise real pause/resume and resume-of-resume chains.
pub fn run_query_segment(
    inv: &Invocation,
    request: &QuerySegmentRequest<'_>,
) -> std::io::Result<QuerySegmentRecord> {
    validate_invocation_budget(inv)?;
    validate_segment_request(request)?;
    let before = trace_files(request.workspace)?;
    let mut cmd = Command::new(&inv.ferric_bin);
    cmd.args(query_segment_args(inv, request));
    cmd.stdin(std::process::Stdio::null());
    if inv.prompts_dir.is_none() {
        cmd.env_remove("FERRIC_PROMPTS_DIR");
    }

    let outcome = run_bounded(
        &mut cmd,
        request.timeout,
        CapturePlan::stderr_tail(STDERR_TAIL_BYTES),
    )?;
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
        stderr_tail: String::from_utf8_lossy(&outcome.stderr).into_owned(),
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
        append_mock_budget_coordinates(&mut args, inv);
    }
    append_output_cap(&mut args, inv);
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
        append_mock_budget_coordinates(&mut args, inv);
    }
    append_output_cap(&mut args, inv);
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

fn append_mock_budget_coordinates(args: &mut Vec<String>, inv: &Invocation) {
    if let Some(controls) = &inv.budget {
        args.extend([
            "--params-b".to_string(),
            controls.params_b().to_string(),
            "--ctx".to_string(),
            controls.ctx().to_string(),
        ]);
    }
}

fn append_output_cap(args: &mut Vec<String>, inv: &Invocation) {
    if let Some(cap) = inv
        .budget
        .as_ref()
        .and_then(BudgetControls::max_output_tokens)
    {
        args.extend(["--max-output-tokens".to_string(), cap.to_string()]);
    }
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
            budget: None,
        }
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn bench_budget_argv_real_mock_and_resume() {
        for real in [false, true] {
            let mut inv = base();
            inv.budget = Some(BudgetControls::new(0.5, Some(4096), 7.0, 32768).unwrap());
            inv.prompts_dir = Some(PathBuf::from("prompts with spaces ' &"));
            if real {
                inv.openai = Some(OpenAiArgs {
                    api_base: Some("http://127.0.0.1:8080/v1".into()),
                    model: "model with spaces".into(),
                    params_b: 7.0,
                    ctx: 32768,
                });
            }
            validate_invocation_budget(&inv).unwrap();
            let initial = query_args(
                "task",
                12,
                &inv,
                Path::new("workspace ' &"),
                Path::new("profiles ' &"),
            );
            let request = QuerySegmentRequest {
                workspace: Path::new("workspace ' &"),
                profile_dir: Path::new("profiles ' &"),
                checks_file: Some(Path::new("checks ' &.toml")),
                prompt: None,
                resume: Some(Path::new("prior ' &.jsonl")),
                answer: Some("answer ' &"),
                max_turns: 7,
                timeout: Duration::from_secs(17),
                api_base_override: Some("http://127.0.0.1:9/v1"),
                harness_policy: Some(HarnessPolicy::Evidence),
            };
            let resumed = query_segment_args(&inv, &request);
            for args in [&initial, &resumed] {
                assert!(has_pair(args, "--params-b", "7"));
                assert!(has_pair(args, "--ctx", "32768"));
                assert!(has_pair(args, "--max-output-tokens", "4096"));
                assert_eq!(
                    args.iter()
                        .filter(|arg| *arg == "--max-output-tokens")
                        .count(),
                    1
                );
                assert_eq!(args.iter().filter(|arg| *arg == "--ctx").count(), 1);
                assert_eq!(args.iter().filter(|arg| *arg == "--params-b").count(), 1);
                assert!(has_pair(args, "--profile-dir", "profiles ' &"));
                assert!(has_pair(args, "--workspace", "workspace ' &"));
                assert!(has_pair(args, "--temperature", "0.0"));
                assert!(has_pair(args, "--protocol", "grammar"));
                assert!(args.iter().any(|arg| arg == "--no-config"));
            }
            assert!(has_pair(
                &initial,
                "--prompts-dir",
                "prompts with spaces ' &"
            ));
            assert!(!resumed.iter().any(|arg| arg == "--prompts-dir"));
            assert!(has_pair(&resumed, "--resume", "prior ' &.jsonl"));
            assert!(has_pair(&resumed, "--answer", "answer ' &"));
            assert!(has_pair(&resumed, "--checks-file", "checks ' &.toml"));
            assert!(has_pair(&resumed, "--harness-policy", "evidence"));
            if real {
                assert!(has_pair(&resumed, "--api-base", "http://127.0.0.1:9/v1"));
            } else {
                assert!(resumed.iter().any(|arg| arg == "--mock"));
            }
            assert_eq!(
                request.timeout,
                Duration::from_secs(17),
                "segment fixture budget is not scaled"
            );
        }
    }

    #[test]
    fn legacy_continuation_argv_unchanged() {
        let inv = base();
        let request = QuerySegmentRequest {
            workspace: Path::new("/ws"),
            profile_dir: Path::new("/profiles"),
            checks_file: None,
            prompt: None,
            resume: Some(Path::new("/prior.jsonl")),
            answer: Some("continue"),
            max_turns: 9,
            timeout: Duration::from_secs(17),
            api_base_override: None,
            harness_policy: None,
        };
        assert_eq!(
            query_segment_args(&inv, &request),
            [
                "query",
                "--resume",
                "/prior.jsonl",
                "--answer",
                "continue",
                "--workspace",
                "/ws",
                "--protocol",
                "grammar",
                "--no-config",
                "--profile-dir",
                "/profiles",
                "--temperature",
                "0.0",
                "--max-turns",
                "9",
                "--no-stream",
                "--mock",
            ]
        );
        assert_eq!(
            query_args("task", 9, &inv, Path::new("/ws"), Path::new("/profiles")),
            [
                "query",
                "task",
                "--workspace",
                "/ws",
                "--protocol",
                "grammar",
                "--no-config",
                "--profile-dir",
                "/profiles",
                "--temperature",
                "0.0",
                "--max-turns",
                "9",
                "--no-stream",
                "--mock",
            ]
        );
    }

    #[test]
    fn mismatched_pre_resolved_budget_rejects_before_workspace_or_child() {
        let spec = crate::embedded_specs().unwrap().remove(0);
        let mut inv = base();
        inv.ferric_bin = PathBuf::from("not-a-real-ferric-child");
        let controls = BudgetControls::new(0.5, Some(1024), 7.0, 4096).unwrap();
        let resolved = controls.resolve_agent(spec.timeout_s).unwrap();
        let error = run_spec_with_budget(&spec, &inv, &resolved).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        inv.budget = Some(controls.clone());
        let wrong_spec = controls.resolve_agent(spec.timeout_s + 1).unwrap();
        assert_eq!(
            run_spec_with_budget(&spec, &inv, &wrong_spec)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        inv.openai = Some(OpenAiArgs {
            api_base: None,
            model: "model".into(),
            params_b: 7.0,
            ctx: 8192,
        });
        assert_eq!(
            run_spec_with_budget(&spec, &inv, &resolved)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    fn run_scaled_source_fixture(
        mode: &str,
    ) -> (tempfile::TempDir, crate::budget::AttemptBudgetEvidence) {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("source.jsonl");
        let started = dir.path().join("started");
        let budget = BudgetControls::new(1.0 / 30.0, Some(1024), 7.0, 4096)
            .unwrap()
            .resolve_agent(60)
            .unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "runner::tests::budget_source_child_fixture",
                "--nocapture",
            ])
            .env("FERRIC_BUDGET_FIXTURE_MODE", mode)
            .env("FERRIC_BUDGET_FIXTURE_TRACE", &trace)
            .env("FERRIC_BUDGET_FIXTURE_STARTED", &started);
        let outcome = run_bounded(
            &mut command,
            budget.duration(),
            CapturePlan::stderr_tail(STDERR_TAIL_BYTES),
        )
        .unwrap();
        assert!(outcome.timed_out);
        assert!(outcome.wall >= budget.duration());
        assert!(
            started.is_file(),
            "source fixture must actually reach its stalled phase"
        );
        if mode == "partial-noisy" {
            assert_eq!(outcome.stderr.len(), STDERR_TAIL_BYTES);
            assert!(outcome.stderr.iter().all(|byte| *byte == b'x'));
        }
        let mut evidence = budget.evidence(outcome.exit_code, outcome.timed_out);
        evidence
            .observe_trace(trace.is_file().then_some(trace.as_path()))
            .unwrap();
        // run_bounded has synchronously completed checked cleanup before this
        // successful return. No manual process repair is permitted here.
        (dir, evidence)
    }

    #[test]
    fn bench_early_timeout_retains_parent_budget() {
        let (_dir, evidence) = run_scaled_source_fixture("no-trace");
        assert_eq!(evidence.base_timeout_s, 60);
        assert_eq!(evidence.controls.timeout_scale(), 1.0 / 30.0);
        assert_eq!(
            evidence.enforced_duration,
            crate::budget::ExactDuration { secs: 2, nanos: 0 }
        );
        assert_eq!(
            evidence.parent_termination,
            crate::budget::ParentTermination::ExecutionTimeout
        );
        assert_eq!(
            evidence.trace,
            crate::budget::TraceBudgetObservation::missing()
        );
        assert!(evidence.retained.is_none());
    }

    #[test]
    fn scaled_deadline_owns_checked_cleanup() {
        let (dir, evidence) = run_scaled_source_fixture("partial-noisy");
        assert!(matches!(
            evidence.trace.state,
            crate::budget::TraceEvidenceState::Malformed { .. }
        ));
        assert!(evidence.trace.main_action_budgets.is_none());
        assert!(evidence.trace.child_terminal.is_none());
        let reference = crate::retain_budget_trace(
            &dir.path().join("source.jsonl"),
            &dir.path().join("retained"),
            crate::budget::AttemptIdentity::new("run-timeout", "trial-001", 0).unwrap(),
            &evidence,
        )
        .unwrap();
        let sidecar = crate::verify_budget_trace(&dir.path().join("retained"), &reference).unwrap();
        assert_eq!(
            sidecar.evidence.parent_termination,
            crate::budget::ParentTermination::ExecutionTimeout
        );
        assert_eq!(sidecar.evidence.trace, evidence.trace);
    }

    #[test]
    #[ignore = "finite source child mode, invoked only by the checked Cargo parent tests"]
    fn budget_source_child_fixture() {
        let Ok(mode) = std::env::var("FERRIC_BUDGET_FIXTURE_MODE") else {
            return;
        };
        std::fs::write(
            std::env::var_os("FERRIC_BUDGET_FIXTURE_STARTED").unwrap(),
            b"started",
        )
        .unwrap();
        if mode == "partial-noisy" {
            std::fs::write(std::env::var_os("FERRIC_BUDGET_FIXTURE_TRACE").unwrap(),
                b"{\"v\":1,\"ts_ms\":1,\"session\":\"child\",\"seq\":0,\"event\":{\"type\":\"main_action_budget\",\"turn\":0,\"budget\":{\"requested\":1024,\"effective\":1024,\"declared_ctx\":4096,\"source\":\"explicit\"}}}\n{\"v\":1").unwrap();
            eprint!("{}", "x".repeat(128 * 1024));
        }
        std::thread::sleep(Duration::from_secs(8));
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
    fn verbose_source_child_cannot_deadlock_file_capture() {
        let mut cmd = noisy_child_command();
        let outcome = run_bounded(
            &mut cmd,
            Duration::from_secs(15),
            CapturePlan::stderr_tail(STDERR_TAIL_BYTES),
        )
        .unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        assert!(!outcome.timed_out);
        assert!(outcome.stderr.ends_with(b"THE-END"));
        assert!(outcome.stderr.len() <= STDERR_TAIL_BYTES);
    }

    fn noisy_child_command() -> Command {
        let mut cmd = Command::new(std::env::current_exe().expect("current test executable"));
        cmd.args([
            "--ignored",
            "--exact",
            "runner::tests::noisy_child_fixture",
            "--nocapture",
        ]);
        cmd
    }

    #[test]
    #[ignore = "test-only source child fixture"]
    fn noisy_child_fixture() {
        print!("{}", "x".repeat(128 * 1024));
        eprint!("{}THE-END", "x".repeat(128 * 1024));
    }
}
