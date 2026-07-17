//! T-3704 integration tests: threading the display sink through the agent
//! loop (ADR-047). `stream_sink: None` byte-identical regression is proven
//! for free by every OTHER test file in this crate continuing to pass
//! unchanged (each call site now passes `None` explicitly, no behavior
//! change) — this file covers the `Some` path.

mod common;

use std::sync::Mutex;

use async_trait::async_trait;
use common::*;
use ferric_guard::Workspace;
use ferric_loop::{RunArgs, StopReason, run};
use ferric_provider::{
    Capabilities, Completion, CompletionRequest, Provider, ProviderError, SamplingParams,
    StreamDelta,
};
use ferric_tools::{Registry, register_builtin_tools};
use serde_json::json;

/// One scripted `complete_streaming` call: the deltas to fire, then the
/// result to return.
type ScriptedStep = (Vec<StreamDelta>, Result<Completion, ProviderError>);

/// A provider whose `complete_streaming` is scripted: each call fires a fixed
/// sequence of deltas, then returns a fixed result (success or error). Never
/// overrides `complete()` — these tests exercise the streaming path only.
struct ScriptedStreamingProvider {
    script: Mutex<Vec<ScriptedStep>>,
}

impl ScriptedStreamingProvider {
    fn new(script: Vec<ScriptedStep>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().rev().collect()),
        }
    }
}

#[async_trait]
impl Provider for ScriptedStreamingProvider {
    fn id(&self) -> &str {
        "scripted-streaming"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_native_tool_calls: false,
            supports_constraint: true,
            exposes_logits: false,
            supports_media: false,
        }
    }

    async fn complete(&self, _request: CompletionRequest, _cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>) -> Result<Completion, ProviderError> {
        unreachable!("test provider only exercises complete_streaming")
    }

    async fn complete_streaming(
        &self,
        _request: CompletionRequest,
        on_delta: &(dyn Fn(StreamDelta) + Sync),
        _cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        let (deltas, result) = self
            .script
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| (Vec::new(), Err(ProviderError::ScriptExhausted(0))));
        for d in deltas {
            on_delta(d);
        }
        result
    }
}

fn task_complete_completion(summary: &str) -> Completion {
    json_completion(json!({"thought": "...", "tool": "task_complete", "args": {"summary": summary}}))
}

/// The turn loop, given `stream_sink: Some`, calls `complete_streaming` and
/// dispatches off the resulting `Completion` exactly as a non-streaming turn
/// would — proving streaming doesn't disturb dispatch/validation logic.
#[test]
fn stream_sink_some_drives_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = ferric_trace::JsonlSink::open(&trace_path, "s-1").unwrap();

    let provider = ScriptedStreamingProvider::new(vec![(
        vec![
            StreamDelta::ToolNamed("task_complete".to_string()),
            StreamDelta::Text("done".to_string()),
        ],
        Ok(task_complete_completion("done")),
    )]);

    let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
    let on_delta = |d: StreamDelta| deltas.lock().unwrap().push(d);
    let sleeper = RecordingSleeper::new();

    let outcome = futures_executor::block_on(run(
        RunArgs {
            cancel_flag: None,
sink_policy: ferric_guard::SinkPolicy::deny(),
taint_set: ferric_guard::TaintSet::new(),
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ferric_core::ActionProtocol::ConstrainedJson,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: Some(&on_delta),
            resume: None,
        },
        &mut sink,
        Some("do the task"),
    ))
    .unwrap();

    assert_eq!(outcome.stop, StopReason::TaskComplete);
    assert_eq!(outcome.final_text.as_deref(), Some("done"));
    assert_eq!(
        deltas.into_inner().unwrap(),
        vec![
            StreamDelta::ToolNamed("task_complete".to_string()),
            StreamDelta::Text("done".to_string()),
        ]
    );
}

/// T-3704 EARS (plan-critique C-006): a retryable mid-stream error retries
/// with a fresh request; already-fired deltas from the failed attempt are
/// never replayed by the next attempt (each attempt fires its OWN deltas).
#[test]
fn streaming_retry_does_not_replay_failed_attempt_deltas() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = ferric_trace::JsonlSink::open(&trace_path, "s-2").unwrap();

    let provider = ScriptedStreamingProvider::new(vec![
        (
            vec![StreamDelta::Text("attempt1".to_string())],
            Err(ProviderError::RetryableBackend("timeout".to_string())),
        ),
        (
            vec![StreamDelta::Text("attempt2".to_string())],
            Ok(task_complete_completion("recovered")),
        ),
    ]);

    let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
    let on_delta = |d: StreamDelta| deltas.lock().unwrap().push(d);
    let sleeper = RecordingSleeper::new();

    let outcome = futures_executor::block_on(run(
        RunArgs {
            cancel_flag: None,
sink_policy: ferric_guard::SinkPolicy::deny(),
taint_set: ferric_guard::TaintSet::new(),
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ferric_core::ActionProtocol::ConstrainedJson,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: Some(&on_delta),
            resume: None,
        },
        &mut sink,
        Some("do the task"),
    ))
    .unwrap();

    assert_eq!(outcome.stop, StopReason::TaskComplete);
    assert_eq!(outcome.final_text.as_deref(), Some("recovered"));
    // attempt1's delta fired exactly once (not duplicated by the retry), then
    // attempt2's — the final Completion is attempt2's, not a merge.
    assert_eq!(
        deltas.into_inner().unwrap(),
        vec![
            StreamDelta::Text("attempt1".to_string()),
            StreamDelta::Text("attempt2".to_string()),
        ]
    );
    assert_eq!(sleeper.delays_ms(), vec![250]);
}
