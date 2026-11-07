//! T-107 integration tests: exponential backoff on retryable provider errors.

mod common;

use common::*;
use ferric_guard::Workspace;
use ferric_loop::{StopReason, run};
use ferric_provider::{Completion, ProviderError};
use ferric_tools::{Registry, register_builtin_tools};

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

    let provider = ScriptedProvider::new(script.into_iter().map(|r| (Vec::new(), r)).collect());
    let sleeper = RecordingSleeper::new();
    let outcome = futures_executor::block_on(run(
        common::args(&provider, &registry, &workspace, &nano_policy(), &sleeper),
        &mut sink,
        Some("do the task"),
    ))
    .unwrap();
    let records = read_records(&trace_path);
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
