//! T-3904 (sprint 39) integration tests: `RunArgs.resume` threaded into
//! `run()`.

mod common;

use common::*;
use ferric_core::{ActionProtocol, HarnessPolicy, Message};
use ferric_guard::Workspace;
use ferric_loop::{
    ControllerState, ReplayError, ReplayedState, RunArgs, StopReason, replay, run,
    validate_resume_target,
};
use ferric_provider::{MockProvider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};

fn base_replayed(turns: u32, workspace: &std::path::Path) -> ReplayedState {
    ReplayedState {
        messages: vec![
            Message::system("You are Ferric."),
            Message::user_with_media("do the task", Vec::new()),
        ],
        turns,
        next_turn: turns,
        last_text: None,
        protocol: ActionProtocol::NativeTools,
        harness_policy: ferric_core::HarnessPolicy::Legacy,
        truncation_limit: ferric_core::DEFAULT_TRUNCATION_LIMIT,
        source_session: "orig-session".to_string(),
        workspace: workspace.to_path_buf(),
        head_len: 2,
        committed_turn_starts: Vec::new(),
        guard_history: Vec::new(),
        nudged_for_no_action: false,
        truncated_once: false,
        last_input_tokens: None,
        pending_input: None,
        mutation_epoch: 0,
        passed_checks: std::collections::BTreeMap::new(),
        pause_reason: None,
        controller_checkpoint: None,
    }
}

#[test]
fn resume_target_inherits_omitted_policy_and_rejects_an_explicit_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let replayed = base_replayed(0, dir.path());

    validate_resume_target(&replayed, dir.path(), ActionProtocol::NativeTools, None).unwrap();
    validate_resume_target(
        &replayed,
        dir.path(),
        ActionProtocol::NativeTools,
        Some(HarnessPolicy::Legacy),
    )
    .unwrap();

    let error = validate_resume_target(
        &replayed,
        dir.path(),
        ActionProtocol::NativeTools,
        Some(HarnessPolicy::Evidence),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ReplayError::HarnessPolicyMismatch {
            recorded: HarnessPolicy::Legacy,
            requested: HarnessPolicy::Evidence,
        }
    ));
}

#[test]
fn unavailable_policies_write_no_event_and_dispatch_no_tool() {
    // `Evidence` is now a live policy (sprint 113); only `EvidencePlanner`
    // remains unavailable and must still be refused by `run()` before any trace
    // event or tool dispatch. The evidence-runs path is covered positively by
    // ferric-cli's `evidence_runs_and_planner_fails_before_trace_or_workspace_mutation`.
    let policy = HarnessPolicy::EvidencePlanner;
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join(format!("{}.jsonl", policy.label()));
    let mut sink = JsonlSink::open(&trace_path, "refused-policy").unwrap();
    let provider = MockProvider::new(vec![tool_completion(vec![(
        "tc-0",
        "write_file",
        serde_json::json!({"path": "should-not-exist.txt", "content": "no"}),
    )])]);
    let sleeper = RecordingSleeper::new();

    let error = futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            provenance: ferric_guard::Provenance::Clean,
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            harness_policy: Some(policy),
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
        Some("write a file"),
    ))
    .unwrap_err();

    assert!(error.to_string().contains(policy.label()), "{error}");
    drop(sink);
    assert_eq!(std::fs::read_to_string(&trace_path).unwrap(), "");
    assert!(!dir.path().join("should-not-exist.txt").exists());
    assert!(provider.requests().is_empty());
}

#[test]
fn evidence_resume_rejects_required_check_drift_before_trace_or_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let registry = Registry::new();
    let trace_path = dir.path().join("required-check-mismatch.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "required-check-mismatch").unwrap();
    let provider = MockProvider::new(vec![text_completion("must not run")]);
    let sleeper = RecordingSleeper::new();
    let mut replayed = base_replayed(0, dir.path());
    replayed.harness_policy = HarnessPolicy::Evidence;
    replayed.pause_reason = Some("max_turns".to_string());
    replayed.controller_checkpoint = Some(
        ControllerState::new(HarnessPolicy::Evidence, ["unit".to_string()])
            .unwrap()
            .checkpoint_for_pause("max_turns")
            .unwrap(),
    );

    let error = futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            provenance: ferric_guard::Provenance::Clean,
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
            hooks: None,
        },
        &mut sink,
        None,
    ))
    .unwrap_err();

    assert!(
        error.to_string().contains("recorded required checks"),
        "{error}"
    );
    drop(sink);
    assert_eq!(std::fs::read_to_string(&trace_path).unwrap(), "");
    assert!(provider.requests().is_empty());
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
            provenance: ferric_guard::Provenance::Clean,
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
            resume: Some(base_replayed(5, dir.path())),
            answer: None,
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
            provenance: ferric_guard::Provenance::Clean,
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
            resume: Some(base_replayed(0, dir.path())),
            answer: None,
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
            provenance: ferric_guard::Provenance::Clean,
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
            provenance: ferric_guard::Provenance::Clean,
            provider: &provider1,
            registry: &registry1,
            workspace: &workspace1,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &sleeper1,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            hooks: None,
        },
        &mut sink1,
        Some("write a.txt"),
    ))
    .unwrap();
    assert_eq!(first.stop, StopReason::TaskComplete);

    // 2. Simulate a kill immediately before the task-complete commit barrier.
    //    Dropping SessionEnd and TurnCommitted leaves only a side-effect-free
    //    intercepted control tail, which replay safely discards and retries.
    let content = std::fs::read_to_string(&trace_path).unwrap();
    let mut lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.pop().map(|l| l.contains("session_end")), Some(true));
    assert_eq!(
        lines.pop().map(|l| l.contains("turn_committed")),
        Some(true)
    );
    std::fs::write(&trace_path, lines.join("\n") + "\n").unwrap();

    // 3. replay() the truncated REAL trace.
    let replayed = replay(&trace_path).unwrap();
    assert_eq!(replayed.protocol, ActionProtocol::NativeTools);
    assert_eq!(replayed.harness_policy, HarnessPolicy::Legacy);

    // 4. A second real run, resuming, that finishes the task.
    let mut registry2 = Registry::new();
    register_builtin_tools(&mut registry2);
    let trace_path2 = dir1.path().join("resume-trace.jsonl");
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
            provenance: ferric_guard::Provenance::Clean,
            provider: &provider2,
            registry: &registry2,
            workspace: &workspace1,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &sleeper2,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(replayed),
            answer: None,
            hooks: None,
        },
        &mut sink2,
        None,
    ))
    .unwrap();

    assert_eq!(second.stop, StopReason::TaskComplete);
    assert_eq!(second.final_text.as_deref(), Some("wrote a.txt"));
}
