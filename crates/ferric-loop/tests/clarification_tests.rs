mod common;

use common::*;
use ferric_core::ActionProtocol;
use ferric_guard::Workspace;
use ferric_loop::{RunArgs, StopReason, replay, run};
use ferric_provider::{MockProvider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};
use serde_json::json;

fn request_args() -> serde_json::Value {
    json!({
        "question": "Which database should this target?",
        "context": "Both adapters exist and the repository documents no default.",
        "options": ["SQLite", "PostgreSQL"]
    })
}

#[test]
fn clarification_pause_is_durable_and_answer_resumes_the_original_objective() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let policy = nano_policy();

    let first_trace = dir.path().join("clarification.jsonl");
    let mut first_sink = JsonlSink::open(&first_trace, "clarify-1").unwrap();
    let first_provider = MockProvider::new(vec![tool_completion(vec![(
        "ask-1",
        ferric_loop::REQUEST_USER_INPUT,
        request_args(),
    )])]);
    let first_sleeper = RecordingSleeper::new();
    let first = futures_executor::block_on(run(
        RunArgs {
            provider: &first_provider,
            registry: &registry,
            workspace: &workspace,
            policy: &policy,
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &first_sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            cancel_flag: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        },
        &mut first_sink,
        Some("Resolve the database ambiguity, then finish the task."),
    ))
    .unwrap();
    drop(first_sink);

    assert_eq!(first.stop, StopReason::NeedsInput);
    assert!(first.final_text.is_none());
    let pending = first.needs_input.as_ref().expect("structured request");
    assert_eq!(pending.continuation_id, "clarify-1");
    assert_eq!(pending.request.options, vec!["SQLite", "PostgreSQL"]);

    let records: Vec<_> = TraceReader::open(&first_trace)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let tail: Vec<&str> = records
        .iter()
        .rev()
        .take(3)
        .map(|record| match &record.event {
            ParsedEvent::Known(Event::SessionEnd { .. }) => "session_end",
            ParsedEvent::Known(Event::RecoveryCheckpoint { .. }) => "checkpoint",
            ParsedEvent::Known(Event::SessionPaused { .. }) => "paused",
            other => panic!("unexpected recovery tail event: {other:?}"),
        })
        .collect();
    assert_eq!(tail, vec!["paused", "checkpoint", "session_end"]);
    assert!(records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::ToolResult {
            name,
            is_error: false,
            ..
        }) if name == ferric_loop::REQUEST_USER_INPUT
    )));

    let replayed = replay(&first_trace).unwrap();
    assert_eq!(replayed.pending_input.as_ref(), Some(&pending.request));

    let second_trace = dir.path().join("answered.jsonl");
    let mut second_sink = JsonlSink::open(&second_trace, "clarify-2").unwrap();
    let second_provider = MockProvider::new(vec![tool_completion(vec![(
        "done-1",
        ferric_loop::TASK_COMPLETE,
        json!({"summary": "targeted SQLite"}),
    )])]);
    let second_sleeper = RecordingSleeper::new();
    let second = futures_executor::block_on(run(
        RunArgs {
            provider: &second_provider,
            registry: &registry,
            workspace: &workspace,
            policy: &policy,
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &second_sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(replayed),
            answer: Some("SQLite"),
            cancel_flag: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        },
        &mut second_sink,
        None,
    ))
    .unwrap();
    drop(second_sink);

    assert_eq!(second.stop, StopReason::TaskComplete);
    assert_eq!(second.final_text.as_deref(), Some("targeted SQLite"));
    let messages = &second_provider.requests()[0].messages;
    let transcript = messages
        .iter()
        .filter_map(|message| message.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(transcript.contains("Resolve the database ambiguity"));
    assert!(transcript.contains("[goal amendment: clarification answer]"));
    assert_eq!(transcript.matches("User answer: SQLite").count(), 1);

    let answered_events: Vec<_> = TraceReader::open(&second_trace)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let kinds: Vec<&str> = answered_events
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::RecoveryCheckpoint { .. }) => Some("checkpoint"),
            ParsedEvent::Known(Event::ResumePrompt { .. }) => Some("answer"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec!["checkpoint", "answer", "checkpoint"]);

    // Crash after the durable answer but before its self-contained anchor.
    // Replay must retain the answer exactly once and consume the request, so a
    // resume-of-resume does not force or duplicate a second answer.
    let answered_text = std::fs::read_to_string(&second_trace).unwrap();
    let through_answer = answered_text.lines().collect::<Vec<_>>();
    let answer_index = through_answer
        .iter()
        .position(|line| line.contains("\"type\":\"resume_prompt\""))
        .expect("answered trace contains ResumePrompt");
    let crashed_answer = dir.path().join("crashed-after-answer.jsonl");
    std::fs::write(
        &crashed_answer,
        through_answer[..=answer_index].join("\n") + "\n",
    )
    .unwrap();

    let replayed_after_answer = replay(&crashed_answer).unwrap();
    assert!(replayed_after_answer.pending_input.is_none());
    let replayed_text = replayed_after_answer
        .messages
        .iter()
        .filter_map(|message| message.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(replayed_text.matches("User answer: SQLite").count(), 1);

    let third_trace = dir.path().join("resume-after-answer-crash.jsonl");
    let mut third_sink = JsonlSink::open(&third_trace, "clarify-3").unwrap();
    let third_provider = MockProvider::new(vec![tool_completion(vec![(
        "done-2",
        ferric_loop::TASK_COMPLETE,
        json!({"summary": "continued without a duplicate answer"}),
    )])]);
    let third_sleeper = RecordingSleeper::new();
    let third = futures_executor::block_on(run(
        RunArgs {
            provider: &third_provider,
            registry: &registry,
            workspace: &workspace,
            policy: &policy,
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &third_sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(replayed_after_answer),
            answer: None,
            cancel_flag: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        },
        &mut third_sink,
        None,
    ))
    .unwrap();
    assert_eq!(third.stop, StopReason::TaskComplete);
}

#[test]
fn committed_clarification_recovers_pending_input_before_terminal_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace = dir.path().join("source.jsonl");
    let mut sink = JsonlSink::open(&trace, "crash-after-commit").unwrap();
    let provider = MockProvider::new(vec![tool_completion(vec![(
        "ask-1",
        ferric_loop::REQUEST_USER_INPUT,
        request_args(),
    )])]);
    let sleeper = RecordingSleeper::new();
    let outcome = futures_executor::block_on(run(
        RunArgs {
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            cancel_flag: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        },
        &mut sink,
        Some("ask if needed"),
    ))
    .unwrap();
    assert_eq!(outcome.stop, StopReason::NeedsInput);
    drop(sink);

    let content = std::fs::read_to_string(&trace).unwrap();
    let through_commit = content
        .lines()
        .take_while(|line| !line.contains("\"type\":\"session_end\""))
        .collect::<Vec<_>>()
        .join("\n");
    let crashed = dir.path().join("crashed.jsonl");
    std::fs::write(&crashed, through_commit + "\n").unwrap();

    let replayed = replay(&crashed).unwrap();
    assert_eq!(
        replayed
            .pending_input
            .as_ref()
            .map(|request| request.question.as_str()),
        Some("Which database should this target?")
    );
}

#[test]
fn mixed_clarification_turn_executes_no_sibling_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace = dir.path().join("mixed.jsonl");
    let mut sink = JsonlSink::open(&trace, "mixed-request").unwrap();
    let provider = MockProvider::new(vec![
        tool_completion(vec![
            ("ask-1", ferric_loop::REQUEST_USER_INPUT, request_args()),
            (
                "write-1",
                "write_file",
                json!({"path": "must-not-exist.txt", "content": "unsafe"}),
            ),
        ]),
        tool_completion(vec![(
            "done-1",
            ferric_loop::TASK_COMPLETE,
            json!({"summary": "recovered"}),
        )]),
    ]);
    let sleeper = RecordingSleeper::new();
    let outcome = futures_executor::block_on(run(
        RunArgs {
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            cancel_flag: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        },
        &mut sink,
        Some("finish safely"),
    ))
    .unwrap();
    drop(sink);

    assert_eq!(outcome.stop, StopReason::TaskComplete);
    assert!(!dir.path().join("must-not-exist.txt").exists());
    let error_results = TraceReader::open(&trace)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|record| {
            matches!(
                &record.event,
                ParsedEvent::Known(Event::ToolResult { is_error: true, .. })
            )
        })
        .count();
    assert_eq!(error_results, 2);
}
