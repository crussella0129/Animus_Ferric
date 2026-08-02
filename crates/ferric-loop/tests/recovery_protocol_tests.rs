mod common;

use common::*;
use ferric_core::{ActionProtocol, Message, ToolCall};
use ferric_guard::Workspace;
use ferric_loop::{ReplayError, ReplayedState, RunArgs, StopReason, replay, run};
use ferric_provider::{MockProvider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink};
use serde_json::json;

fn policy() -> Event {
    Event::PolicySelected {
        tier: ferric_core::Tier::Nano,
        protocol: ActionProtocol::NativeTools,
        harness_policy: ferric_core::HarnessPolicy::Legacy,
        max_turns: 15,
        max_tools: 10,
        prompt_budget_tokens: 2_800,
        max_output_tokens: 512,
        truncation_limit: ferric_core::DEFAULT_TRUNCATION_LIMIT,
        tier_source: ferric_core::TierSource::Params.label().to_string(),
    }
}

fn write_modern_tail(
    path: &std::path::Path,
    workspace: &std::path::Path,
    include_call: bool,
    include_result: bool,
) {
    let mut sink = JsonlSink::open(path, "recovery-tail").unwrap();
    let call = ToolCall {
        id: "write-1".to_string(),
        name: "write_file".to_string(),
        args: json!({"path": "a.txt", "content": "hello"}),
    };
    for event in [
        Event::SessionStart {
            workspace: workspace.display().to_string(),
            resumed_from: None,
        },
        policy(),
        Event::SessionPrompt {
            system: "system".to_string(),
            user: "task".to_string(),
            media: Vec::new(),
        },
        Event::TurnStart { turn: 0 },
        Event::TurnEnd {
            turn: 0,
            text: None,
            tool_call_count: 1,
            input_tokens: Some(20),
            output_tokens: Some(5),
            truncated: false,
        },
        Event::ActionsProposed {
            turn: 0,
            calls: vec![call.clone()],
        },
    ] {
        sink.write_event(event).unwrap();
    }
    if include_call {
        sink.write_event(Event::ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.args.clone(),
        })
        .unwrap();
    }
    if include_result {
        sink.write_event(Event::ToolResult {
            id: call.id,
            name: call.name,
            output: "wrote 5 bytes to a.txt".to_string(),
            is_error: false,
            duration_ms: 1,
        })
        .unwrap();
    }
}

#[test]
fn dispatched_tail_without_result_fails_closed_as_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("ambiguous.jsonl");
    write_modern_tail(&trace, dir.path(), true, false);

    let error = replay(&trace).unwrap_err();
    assert!(matches!(error, ReplayError::AmbiguousTail { turn: 0, .. }));
    assert!(error.to_string().contains("write_file"));
}

#[test]
fn fully_resulted_tail_is_recovered_without_reexecuting_it() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("resulted.jsonl");
    write_modern_tail(&trace, dir.path(), true, true);

    let state = replay(&trace).unwrap();
    assert_eq!(state.next_turn, 1);
    assert_eq!(state.turns, 1);
    assert!(state.messages.iter().any(|message| {
        message
            .text
            .as_deref()
            .is_some_and(|text| text.contains("wrote 5 bytes"))
    }));
}

#[test]
fn proposal_before_dispatch_is_safely_retried() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("predispatch.jsonl");
    write_modern_tail(&trace, dir.path(), false, false);

    let state = replay(&trace).unwrap();
    assert_eq!(state.next_turn, 0);
    assert_eq!(state.messages.len(), 2);
}

#[test]
fn a_later_turn_cannot_overwrite_an_uncommitted_modern_turn() {
    for include_result in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir
            .path()
            .join(format!("overwritten-{include_result}.jsonl"));
        write_modern_tail(&trace, dir.path(), true, include_result);
        let mut sink = JsonlSink::open(&trace, "recovery-tail").unwrap();
        sink.write_event(Event::TurnStart { turn: 1 }).unwrap();
        drop(sink);

        let error = replay(&trace).unwrap_err();
        assert!(
            matches!(error, ReplayError::InvalidStructure(_)),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("without TurnCommitted"),
            "{error}"
        );
    }
}

#[test]
fn checkpoint_cannot_erase_an_active_or_ambiguous_turn() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("checkpoint-erases-tail.jsonl");
    write_modern_tail(&trace, dir.path(), true, false);
    let checkpoint = ferric_trace::RecoveryCheckpointV1 {
        version: ferric_trace::RECOVERY_CHECKPOINT_VERSION,
        messages: vec![Message::system("system"), Message::user("task")],
        next_turn: 0,
        last_text: None,
        head_len: 2,
        committed_turn_starts: Vec::new(),
        guard_history: Vec::new(),
        nudged_for_no_action: false,
        truncated_once: false,
        last_input_tokens: None,
        pending_input: None,
        mutation_epoch: 0,
        passed_checks: std::collections::BTreeMap::new(),
    };
    let mut sink = JsonlSink::open(&trace, "recovery-tail").unwrap();
    sink.write_event(Event::RecoveryCheckpoint { state: checkpoint })
        .unwrap();
    drop(sink);

    let error = replay(&trace).unwrap_err();
    assert!(matches!(error, ReplayError::InvalidStructure(_)));
    assert!(
        error.to_string().contains("inside an active turn"),
        "{error}"
    );
}

#[test]
fn successful_terminal_commit_is_not_resumable_if_session_end_was_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("success-before-session-end.jsonl");
    let done = ToolCall {
        id: "done-1".to_string(),
        name: ferric_loop::TASK_COMPLETE.to_string(),
        args: json!({"summary": "done"}),
    };
    let mut sink = JsonlSink::open(&trace, "terminal-crash").unwrap();
    for event in [
        Event::SessionStart {
            workspace: dir.path().display().to_string(),
            resumed_from: None,
        },
        policy(),
        Event::SessionPrompt {
            system: "system".to_string(),
            user: "task".to_string(),
            media: Vec::new(),
        },
        Event::TurnStart { turn: 0 },
        Event::TurnEnd {
            turn: 0,
            text: None,
            tool_call_count: 1,
            input_tokens: None,
            output_tokens: None,
            truncated: false,
        },
        Event::ActionsProposed {
            turn: 0,
            calls: vec![done.clone()],
        },
        Event::ToolCall {
            id: done.id,
            name: done.name,
            args: done.args,
        },
        Event::TurnCommitted {
            turn: 0,
            dispatched: 0,
            errored: 0,
            stop_reason: Some("task_complete".to_string()),
            snapshot_commit: None,
        },
    ] {
        sink.write_event(event).unwrap();
    }
    drop(sink);

    let error = replay(&trace).unwrap_err();
    assert!(matches!(
        error,
        ReplayError::AlreadyStopped(ref reason) if reason == "task_complete"
    ));
}

#[test]
fn resume_uses_a_fresh_budget_but_keeps_absolute_turn_ids() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let replayed = ReplayedState {
        messages: vec![Message::system("system"), Message::user("task")],
        turns: 15,
        next_turn: 15,
        last_text: None,
        protocol: ActionProtocol::NativeTools,
        harness_policy: ferric_core::HarnessPolicy::Legacy,
        truncation_limit: ferric_core::DEFAULT_TRUNCATION_LIMIT,
        source_session: "prior".to_string(),
        workspace: dir.path().to_path_buf(),
        head_len: 2,
        committed_turn_starts: Vec::new(),
        guard_history: Vec::new(),
        nudged_for_no_action: false,
        truncated_once: false,
        last_input_tokens: None,
        pending_input: None,
        mutation_epoch: 0,
        passed_checks: std::collections::BTreeMap::new(),
        pause_reason: Some("max_turns".to_string()),
        controller_checkpoint: None,
    };
    let provider = MockProvider::new(vec![tool_completion(vec![(
        "done-15",
        ferric_loop::TASK_COMPLETE,
        json!({"summary": "finished after resume"}),
    )])]);
    let sleeper = RecordingSleeper::new();
    let trace = dir.path().join("resume.jsonl");
    let mut sink = JsonlSink::open(&trace, "resumed").unwrap();
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
            resume: Some(replayed),
            answer: None,
            cancel_flag: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        },
        &mut sink,
        None,
    ))
    .unwrap();

    assert_eq!(outcome.stop, StopReason::TaskComplete);
    assert_eq!(outcome.turns, 16);
    let request = &provider.requests()[0];
    assert!(
        request
            .messages
            .iter()
            .any(|message| { message.text.as_deref() == Some("task") })
    );
}
