//! Shared helpers for ferric-loop integration tests.
//!
//! Compiled once per test binary; each binary uses a different subset of
//! helpers, so dead-code lints are suppressed module-wide.
#![allow(dead_code)]

use std::sync::Mutex;
use std::time::Duration;

use ferric_core::{ActionProtocol, Message, ModelProfile, Role, RunPolicy, ToolCall, policy_for};
use ferric_guard::Workspace;
use ferric_loop::{LoopOutcome, RunArgs, Sleeper, run};
use ferric_provider::{Completion, MockProvider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, ParsedEvent, TraceReader, TraceRecord};

pub fn nano_policy() -> RunPolicy {
    policy_for(&ModelProfile {
        params_b: 1.0,
        quant: "Q4_K_M".to_string(),
        ctx: 4096,
        family: "test".to_string(),
        measured_level: None,
    })
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
        },
        input_tokens: Some(60),
        output_tokens: Some(15),
        truncated: false,
    }
}

/// A grammar-mode completion: the whole assistant text IS the action JSON,
/// tool_calls empty. Used by the UnifiedGrammar loop tests (T-207).
pub fn grammar_completion(action_json: serde_json::Value) -> Completion {
    let name = action_json["tool"].as_str().unwrap_or("");
    let args = action_json["args"].to_string();
    let text = format!("<thought>thinking</thought>\n<tool_call><name>{}</name><args>{}</args></tool_call>", name, args);
    Completion {
        message: Message::assistant(text),
        input_tokens: Some(60),
        output_tokens: Some(20),
        truncated: false,
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

/// Drive the loop with a scripted mock in a temp workspace; return outcome,
/// parsed trace, and recorded sleeps. The provider is returned to the caller
/// via the `inspect` closure before the run is dropped.
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
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = ferric_trace::JsonlSink::open(&trace_path, "t-1").unwrap();

    let provider = MockProvider::new(script);
    let sleeper = RecordingSleeper::new();
    let outcome = futures_executor::block_on(run(
        RunArgs {
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy,
            protocol,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
        },
        &mut sink,
        "do the task",
    ))
    .unwrap();
    inspect(&provider);

    let records: Vec<TraceRecord> = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    RunResult {
        outcome,
        records,
        sleeper_delays_ms: sleeper.delays_ms(),
    }
}

/// Event-kind labels in trace order, for golden-order assertions.
pub fn kinds(records: &[TraceRecord]) -> Vec<&'static str> {
    records
        .iter()
        .map(|r| match &r.event {
            ParsedEvent::Known(Event::SessionStart { .. }) => "session_start",
            ParsedEvent::Known(Event::SessionEnd { .. }) => "session_end",
            ParsedEvent::Known(Event::PolicySelected { .. }) => "policy_selected",
            ParsedEvent::Known(Event::PromptComposed { .. }) => "prompt_composed",
            ParsedEvent::Known(Event::TurnStart { .. }) => "turn_start",
            ParsedEvent::Known(Event::TurnEnd { .. }) => "turn_end",
            ParsedEvent::Known(Event::PromptAssembled { .. }) => "prompt_assembled",
            ParsedEvent::Known(Event::ConstraintApplied { .. }) => "constraint_applied",
            ParsedEvent::Known(Event::RepetitionGuard { .. }) => "repetition_guard",
            ParsedEvent::Known(Event::PermissionCheck { .. }) => "permission_check",
            ParsedEvent::Known(Event::ToolCall { .. }) => "tool_call",
            ParsedEvent::Known(Event::ToolResult { .. }) => "tool_result",
            ParsedEvent::Known(Event::Note { .. }) => "note",
            ParsedEvent::Unknown(_) => "unknown",
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
