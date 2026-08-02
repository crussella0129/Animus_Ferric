//! T-107 integration tests: exponential backoff on retryable provider errors.

mod common;

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use common::*;
use ferric_guard::Workspace;
use ferric_loop::{RunArgs, StopReason, run};
use ferric_provider::{
    Capabilities, Completion, CompletionRequest, Provider, ProviderError, SamplingParams,
};
use ferric_tools::{Registry, register_builtin_tools};

/// Scripted provider that can fail: replays Results instead of Completions.
struct FlakyProvider {
    script: Mutex<VecDeque<Result<Completion, ProviderError>>>,
}

impl FlakyProvider {
    fn new(script: Vec<Result<Completion, ProviderError>>) -> Self {
        Self {
            script: Mutex::new(script.into()),
        }
    }
}

#[async_trait]
impl Provider for FlakyProvider {
    fn id(&self) -> &str {
        "flaky"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: false,
            exposes_logits: false,
            supports_media: false,
        }
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(ProviderError::ScriptExhausted(0)))
    }
}

fn retryable() -> Result<Completion, ProviderError> {
    Err(ProviderError::RetryableBackend("timeout".to_string()))
}

struct FlakyRun {
    outcome: ferric_loop::LoopOutcome,
    delays_ms: Vec<u64>,
    reason: String,
}

fn run_flaky(script: Vec<Result<Completion, ProviderError>>) -> FlakyRun {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = ferric_trace::JsonlSink::open(&trace_path, "b-1").unwrap();

    let provider = FlakyProvider::new(script);
    let sleeper = RecordingSleeper::new();
    let outcome = futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            provenance: ferric_guard::Provenance::Clean,
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ferric_core::ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            hooks: None,
        },
        &mut sink,
        Some("do the task"),
    ))
    .unwrap();
    let records: Vec<_> = ferric_trace::TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    FlakyRun {
        outcome,
        delays_ms: sleeper.delays_ms(),
        reason: session_end_reason(&records),
    }
}

#[test]
fn backoff_schedule() {
    // Three retryable failures, then success: delays must be 250/500/1000
    // and the loop completes normally.
    let result = run_flaky(vec![
        retryable(),
        retryable(),
        retryable(),
        Ok(text_completion("recovered")),
    ]);
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert_eq!(result.outcome.final_text.as_deref(), Some("recovered"));
    assert_eq!(result.delays_ms, vec![250, 500, 1000]);
}

#[test]
fn backoff_exhaustion() {
    // Four retryable failures: retries exhausted after 3 sleeps.
    let result = run_flaky(vec![retryable(), retryable(), retryable(), retryable()]);
    assert_eq!(result.outcome.stop, StopReason::ProviderError);
    assert_eq!(result.delays_ms, vec![250, 500, 1000]);
    assert_eq!(result.reason, "provider_error");
}

#[test]
fn non_retryable_aborts_immediately() {
    let result = run_flaky(vec![Err(ProviderError::Backend(
        "model file corrupt".to_string(),
    ))]);
    assert_eq!(result.outcome.stop, StopReason::ProviderError);
    assert!(result.delays_ms.is_empty(), "no sleeps on permanent errors");
    assert_eq!(result.reason, "provider_error");
}
