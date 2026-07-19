//! T-3904 (sprint 39) integration tests: `RunArgs.resume` threaded into
//! `run()`.

mod common;

use common::*;
use ferric_core::{ActionProtocol, Message};
use ferric_guard::Workspace;
use ferric_loop::{ReplayedState, RunArgs, StopReason, replay, run};
use ferric_provider::{MockProvider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};

fn base_replayed(turns: u32) -> ReplayedState {
    ReplayedState {
        messages: vec![
            Message::system("You are Ferric."),
            Message::user_with_media("do the task", Vec::new()),
        ],
        turns,
        last_text: None,
        protocol: ActionProtocol::NativeTools,
        source_session: "orig-session".to_string(),
    }
}

#[test]
fn resume_some_continues_from_replayed_state() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "resumed-1").unwrap();

    let provider = MockProvider::new(vec![text_completion("all done")]);
    let sleeper = RecordingSleeper::new();
    let outcome = futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            taint_set: ferric_guard::TaintSet::new(),
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(base_replayed(5)),
            hooks: None,
        },
        &mut sink,
        None,
    ))
    .unwrap();

    assert_eq!(outcome.stop, StopReason::FinalText);
    // Continues from the replayed count (5), not reset to 0.
    assert_eq!(outcome.turns, 6);

    let records: Vec<_> = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let session_start = records
        .iter()
        .find_map(|r| match &r.event {
            ParsedEvent::Known(Event::SessionStart { resumed_from, .. }) => {
                Some(resumed_from.clone())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(session_start, Some("orig-session".to_string()));
    assert!(
        !records
            .iter()
            .any(|r| matches!(r.event, ParsedEvent::Known(Event::SessionPrompt { .. }))),
        "a resumed session must not write a new SessionPrompt"
    );
}

#[test]
fn resume_some_with_extra_prompt_appends_one_user_message() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "resumed-2").unwrap();

    let provider = MockProvider::new(vec![text_completion("all done")]);
    let sleeper = RecordingSleeper::new();
    futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            taint_set: ferric_guard::TaintSet::new(),
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(base_replayed(0)),
            hooks: None,
        },
        &mut sink,
        Some("actually, also check b.txt"),
    ))
    .unwrap();

    // The extra prompt is visible to the first (only) request the provider saw.
    let reqs = provider.requests();
    assert!(
        reqs[0].messages.iter().any(|m| m
            .text
            .as_deref()
            .is_some_and(|t| t == "actually, also check b.txt")),
        "expected the extra prompt appended after the replayed history: {:?}",
        reqs[0].messages
    );
}

#[test]
fn resume_none_prompt_none_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "no-prompt").unwrap();

    let provider = MockProvider::new(vec![text_completion("unused")]);
    let sleeper = RecordingSleeper::new();
    let result = futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            taint_set: ferric_guard::TaintSet::new(),
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            hooks: None,
        },
        &mut sink,
        None,
    ));

    assert!(result.is_err());
}

/// Test-critic C-010: a genuine round-trip using a REAL `run()`-produced
/// trace (not another hand-built fixture) — the test most likely to catch
/// drift between what `run()` actually emits and what `replay()` assumes.
#[test]
fn real_run_then_replay_then_resume_reaches_task_complete() {
    // 1. A real run to natural TaskComplete completion (write_file, then
    //    task_complete) — inlined (rather than `run_scripted`, which doesn't
    //    expose its trace path) so the trace file can be truncated below.
    let dir1 = tempfile::tempdir().unwrap();
    let workspace1 = Workspace::new(dir1.path()).unwrap();
    let mut registry1 = Registry::new();
    register_builtin_tools(&mut registry1);
    let trace_path = dir1.path().join("trace.jsonl");
    let mut sink1 = JsonlSink::open(&trace_path, "original-session").unwrap();
    let provider1 = MockProvider::new(vec![
        tool_completion(vec![(
            "tc-0",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "hi"}),
        )]),
        tool_completion(vec![(
            "tc-1",
            ferric_loop::TASK_COMPLETE,
            serde_json::json!({"summary": "wrote a.txt"}),
        )]),
    ]);
    let sleeper1 = RecordingSleeper::new();
    let first = futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            taint_set: ferric_guard::TaintSet::new(),
            provider: &provider1,
            registry: &registry1,
            workspace: &workspace1,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper1,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            hooks: None,
        },
        &mut sink1,
        Some("write a.txt"),
    ))
    .unwrap();
    assert_eq!(first.stop, StopReason::TaskComplete);

    // 2. Simulate a kill: drop the trailing SessionEnd line from the REAL
    //    trace file. (The turn that called task_complete has no further
    //    TurnStart to confirm it, so replay's conservative rule discards it
    //    too — the continuation below simply redoes it.)
    let content = std::fs::read_to_string(&trace_path).unwrap();
    let mut lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.pop().map(|l| l.contains("session_end")), Some(true));
    std::fs::write(&trace_path, lines.join("\n") + "\n").unwrap();

    // 3. replay() the truncated REAL trace.
    let replayed = replay(&trace_path).unwrap();
    assert_eq!(replayed.protocol, ActionProtocol::NativeTools);

    // 4. A second real run, resuming, that finishes the task.
    let dir2 = tempfile::tempdir().unwrap();
    let workspace2 = Workspace::new(dir2.path()).unwrap();
    let mut registry2 = Registry::new();
    register_builtin_tools(&mut registry2);
    let trace_path2 = dir2.path().join("trace.jsonl");
    let mut sink2 = JsonlSink::open(&trace_path2, "resumed-continuation").unwrap();
    let provider2 = MockProvider::new(vec![tool_completion(vec![(
        "tc-2",
        ferric_loop::TASK_COMPLETE,
        serde_json::json!({"summary": "wrote a.txt"}),
    )])]);
    let sleeper2 = RecordingSleeper::new();
    let second = futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            taint_set: ferric_guard::TaintSet::new(),
            provider: &provider2,
            registry: &registry2,
            workspace: &workspace2,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper2,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(replayed),
            hooks: None,
        },
        &mut sink2,
        None,
    ))
    .unwrap();

    assert_eq!(second.stop, StopReason::TaskComplete);
    assert_eq!(second.final_text.as_deref(), Some("wrote a.txt"));
}
