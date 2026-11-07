//! Shared helpers for ferric-loop integration tests.
//!
//! Compiled once per test binary; each binary uses a different subset of
//! helpers, so dead-code lints are suppressed module-wide.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use ferric_core::{ActionProtocol, Message, ModelProfile, Role, RunPolicy, ToolCall, policy_for};
use ferric_guard::{Provenance, SinkPolicy, Workspace};
use ferric_loop::{EditApprover, LoopOutcome, ReplayedState, RunArgs, Sleeper, run};
use ferric_provider::{
    Capabilities, Completion, CompletionRequest, MockProvider, Provider, ProviderError,
    SamplingParams, StreamDelta,
};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, ParsedEvent, TraceReader, TraceRecord};

pub fn nano_policy() -> RunPolicy {
    let mut p = policy_for(&ModelProfile {
        params_b: 1.0,
        quant: "Q4_K_M".to_string(),
        ctx: 4096,
        family: "test".to_string(),
        measured_level: None,
    });
    p.compact_trigger_fraction = 0.85;
    p.compact_keep_last_turns = 2;
    p
}

pub fn text_completion(text: &str) -> Completion {
    Completion {
        message: Message::assistant(text),
        input_tokens: Some(50),
        output_tokens: Some(10),
        truncated: false,
    }
}

/// A completion that hit the token budget (`finish_reason == "length"`).
pub fn truncated_completion() -> Completion {
    Completion {
        message: Message::assistant(
            "{\"tool\":\"write_file\",\"args\":{\"path\":\"a.txt\",\"content\":\"this got cut o",
        ),
        input_tokens: Some(50),
        output_tokens: Some(512),
        truncated: true,
    }
}

pub fn empty_completion() -> Completion {
    Completion {
        message: Message {
            role: Role::Assistant,
            text: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
            media: Vec::new(),
        },
        input_tokens: Some(50),
        output_tokens: Some(0),
        truncated: false,
    }
}

pub fn tool_completion(calls: Vec<(&str, &str, serde_json::Value)>) -> Completion {
    Completion {
        message: Message {
            role: Role::Assistant,
            text: None,
            tool_calls: calls
                .into_iter()
                .map(|(id, name, args)| ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    args,
                })
                .collect(),
            tool_call_id: None,
            media: Vec::new(),
        },
        input_tokens: Some(60),
        output_tokens: Some(15),
        truncated: false,
    }
}

/// A `ConstrainedJson` completion: the whole assistant text IS the action JSON
/// object — exactly what a constraint-honoring backend produces under a
/// JSON-Schema `response_format`. Pass any value to exercise valid actions and
/// non-action objects alike.
pub fn json_completion(value: serde_json::Value) -> Completion {
    Completion {
        message: Message::assistant(value.to_string()),
        input_tokens: Some(60),
        output_tokens: Some(20),
        truncated: false,
    }
}

/// A `TextXml` completion: the action wrapped in a `<tool_call>` XML block,
/// what an unconstrained model is prompted to emit and the loop regex-scrapes.
pub fn xml_completion(action_json: serde_json::Value) -> Completion {
    let name = action_json["tool"].as_str().unwrap_or("");
    let args = action_json["args"].to_string();
    let text = format!(
        "<thought>thinking</thought>\n<tool_call><name>{}</name><args>{}</args></tool_call>",
        name, args
    );
    Completion {
        message: Message::assistant(text),
        input_tokens: Some(60),
        output_tokens: Some(20),
        truncated: false,
    }
}

/// A Legacy resume seed whose whole `messages` history is the protected head,
/// as if `next_turn` turns had already run in session `orig-session`.
pub fn legacy_seed(
    messages: Vec<Message>,
    next_turn: u32,
    workspace: &std::path::Path,
) -> ReplayedState {
    ReplayedState {
        core: ferric_trace::RecoveryCheckpointV1 {
            version: ferric_trace::RECOVERY_CHECKPOINT_VERSION,
            head_len: messages.len(),
            messages,
            next_turn,
            last_text: None,
            committed_turn_starts: Vec::new(),
            guard_history: Vec::new(),
            nudged_for_no_action: false,
            truncated_once: false,
            last_input_tokens: None,
            pending_input: None,
            mutation_epoch: 0,
            passed_checks: std::collections::BTreeMap::new(),
        },
        protocol: ActionProtocol::NativeTools,
        harness_policy: ferric_core::HarnessPolicy::Legacy,
        truncation_limit: ferric_core::DEFAULT_TRUNCATION_LIMIT,
        source_session: "orig-session".to_string(),
        workspace: workspace.to_path_buf(),
        pause_reason: None,
        controller_checkpoint: None,
    }
}

/// Records requested sleep durations instead of sleeping.
pub struct RecordingSleeper {
    pub slept: Mutex<Vec<Duration>>,
}

impl RecordingSleeper {
    pub fn new() -> Self {
        Self {
            slept: Mutex::new(Vec::new()),
        }
    }

    pub fn delays_ms(&self) -> Vec<u64> {
        self.slept
            .lock()
            .unwrap()
            .iter()
            .map(|d| d.as_millis() as u64)
            .collect()
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.slept.lock().unwrap().push(duration);
    }
}

pub struct RunResult {
    pub outcome: LoopOutcome,
    pub records: Vec<TraceRecord>,
    pub sleeper_delays_ms: Vec<u64>,
}

/// `RunArgs` with every optional knob at its default: NativeTools, inherited
/// harness policy, a clean run under a deny sink policy, no approver, no
/// resume. Tests override fields with struct-update syntax.
pub fn args<'a>(
    provider: &'a dyn Provider,
    registry: &'a Registry,
    workspace: &'a Workspace,
    policy: &'a RunPolicy,
    sleeper: &'a dyn Sleeper,
) -> RunArgs<'a> {
    RunArgs {
        provider,
        registry,
        workspace,
        policy,
        protocol: ActionProtocol::NativeTools,
        harness_policy: None,
        sampling: SamplingParams::default(),
        sleeper,
        system_prompt: None,
        prompt_lineage: None,
        media: Vec::new(),
        stream_sink: None,
        resume: None,
        answer: None,
        cancel_flag: None,
        provenance: Provenance::Clean,
        sink_policy: SinkPolicy::deny(),
        hooks: None,
        edit_approver: None,
    }
}

/// Every record of a finished trace, in order.
pub fn read_records(path: impl AsRef<Path>) -> Vec<TraceRecord> {
    TraceReader::open(path.as_ref())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

/// A temp workspace that is a git repository with one commit of `files`, so
/// `vcs.snapshot()` succeeds instead of tracing a failure note every turn.
pub fn git_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let path = dir.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }
    for args in [
        &["init"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "test@example.com"],
        &["add", "."],
        &["commit", "-m", "init"],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    dir
}

/// What a scripted run varies. The default is NativeTools, a clean run under a
/// deny sink policy, and no accept-edits approver.
pub struct Opts<'a> {
    pub protocol: ActionProtocol,
    pub approver: Option<EditApprover<'a>>,
    pub provenance: Provenance,
    pub sink_policy: SinkPolicy,
}

impl Default for Opts<'_> {
    fn default() -> Self {
        Self {
            protocol: ActionProtocol::NativeTools,
            approver: None,
            provenance: Provenance::Clean,
            sink_policy: SinkPolicy::deny(),
        }
    }
}

/// A tainted run (ADR-080) under the default `RequireApproval` sink policy.
pub fn tainted(approver: Option<EditApprover<'_>>) -> Opts<'_> {
    Opts {
        approver,
        provenance: Provenance::UntrustedIngested,
        sink_policy: SinkPolicy::require_approval(),
        ..Opts::default()
    }
}

/// Drive the loop with a scripted mock in a temp git workspace; return
/// outcome, parsed trace, and recorded sleeps.
pub fn run_scripted(
    script: Vec<Completion>,
    policy: &RunPolicy,
    inspect: impl FnOnce(&MockProvider),
) -> RunResult {
    run_scripted_protocol(script, policy, ActionProtocol::NativeTools, inspect)
}

pub fn run_scripted_protocol(
    script: Vec<Completion>,
    policy: &RunPolicy,
    protocol: ActionProtocol,
    inspect: impl FnOnce(&MockProvider),
) -> RunResult {
    let opts = Opts {
        protocol,
        ..Opts::default()
    };
    run_scripted_with(script, policy, opts, inspect, |_| {})
}

/// The one scripted runner. `inspect` sees the provider after the run;
/// `check_dir` runs against the workspace *before* it is torn down, so a test
/// can assert which files were (not) written.
pub fn run_scripted_with(
    script: Vec<Completion>,
    policy: &RunPolicy,
    opts: Opts<'_>,
    inspect: impl FnOnce(&MockProvider),
    check_dir: impl FnOnce(&Path),
) -> RunResult {
    let dir = git_workspace(&[("init.txt", "init")]);
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = ferric_trace::JsonlSink::open(&trace_path, "t-1").unwrap();

    let provider = MockProvider::new(script);
    let sleeper = RecordingSleeper::new();
    let outcome = futures_executor::block_on(run(
        RunArgs {
            protocol: opts.protocol,
            provenance: opts.provenance,
            sink_policy: opts.sink_policy,
            edit_approver: opts.approver,
            ..args(&provider, &registry, &workspace, policy, &sleeper)
        },
        &mut sink,
        Some("do the task"),
    ))
    .unwrap();
    inspect(&provider);
    check_dir(dir.path());

    RunResult {
        outcome,
        records: read_records(&trace_path),
        sleeper_delays_ms: sleeper.delays_ms(),
    }
}

/// An event's trace label: its serde `type` tag, or `unknown`.
pub fn kind(event: &ParsedEvent) -> String {
    match event {
        ParsedEvent::Known(event) => serde_json::to_value(event).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_string(),
        ParsedEvent::Unknown(_) => "unknown".to_string(),
    }
}

/// Event-kind labels in trace order, for golden-order assertions.
pub fn kinds(records: &[TraceRecord]) -> Vec<String> {
    records.iter().map(|r| kind(&r.event)).collect()
}

/// The `warned`/`stopped` actions one guard recorded, in trace order. `guard`
/// is the guard's event kind, e.g. `failure_guard`.
pub fn guard_actions(records: &[TraceRecord], guard: &str) -> Vec<String> {
    records
        .iter()
        .filter(|r| kind(&r.event) == guard)
        .filter_map(|r| match &r.event {
            ParsedEvent::Known(
                Event::RepetitionGuard { action }
                | Event::NoProgressGuard { action }
                | Event::FailureGuard { action }
                | Event::OscillationGuard { action },
            ) => Some(action.clone()),
            _ => None,
        })
        .collect()
}

/// A two-call turn the repetition guard sees as identical when repeated.
pub fn same_calls() -> Vec<(&'static str, &'static str, serde_json::Value)> {
    vec![
        ("a", "list_dir", serde_json::json!({"path": "."})),
        ("b", "read_file", serde_json::json!({"path": "x.txt"})),
    ]
}

/// One scripted provider call: the deltas a streaming call fires, then the
/// result it returns.
pub type ScriptedStep = (Vec<StreamDelta>, Result<Completion, ProviderError>);

/// A provider that replays results, errors included, where `MockProvider`
/// scripts only completions. `complete` ignores a step's deltas;
/// `complete_streaming` fires them first. Past the script it fails
/// non-retryably.
pub struct ScriptedProvider {
    script: Mutex<VecDeque<ScriptedStep>>,
}

impl ScriptedProvider {
    pub fn new(script: Vec<ScriptedStep>) -> Self {
        Self {
            script: Mutex::new(script.into()),
        }
    }

    fn next(&self) -> ScriptedStep {
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| (Vec::new(), Err(ProviderError::ScriptExhausted(0))))
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn id(&self) -> &str {
        "scripted"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: true,
            exposes_logits: false,
            supports_media: false,
        }
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _cancel_flag: Option<Arc<AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        self.next().1
    }

    async fn complete_streaming(
        &self,
        _request: CompletionRequest,
        on_delta: &(dyn Fn(StreamDelta) + Sync),
        _cancel_flag: Option<Arc<AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        let (deltas, result) = self.next();
        for delta in deltas {
            on_delta(delta);
        }
        result
    }
}

/// One `write_file` of `content` to `out.txt`, then a final answer.
pub fn write_then_done(content: &str) -> Vec<Completion> {
    vec![
        tool_completion(vec![(
            "w",
            "write_file",
            serde_json::json!({"path": "out.txt", "content": content}),
        )]),
        text_completion("done"),
    ]
}

/// Did the model receive an error tool result whose text contains `needle`?
pub fn saw_error_result(records: &[TraceRecord], needle: &str) -> bool {
    records.iter().any(|r| {
        matches!(
            &r.event,
            ParsedEvent::Known(Event::ToolResult { output, is_error, .. })
                if *is_error && output.contains(needle)
        )
    })
}

/// `ToolCall` event names in trace order (includes intercepted terminators,
/// which are traced but never dispatched — sprint 39, T-3902).
pub fn tool_call_names(records: &[TraceRecord]) -> Vec<String> {
    records
        .iter()
        .filter_map(|r| match &r.event {
            ParsedEvent::Known(Event::ToolCall { name, .. }) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

pub fn session_end_reason(records: &[TraceRecord]) -> String {
    records
        .iter()
        .rev()
        .find_map(|r| match &r.event {
            ParsedEvent::Known(Event::SessionEnd { reason }) => Some(reason.clone()),
            _ => None,
        })
        .expect("trace must contain session_end")
}
