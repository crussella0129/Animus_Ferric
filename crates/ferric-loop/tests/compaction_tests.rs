//! T-4003/T-4004 (sprint 40) integration tests: `HistoryCompactor` wired into
//! `run()`, and the real compact-kill-replay-resume round-trip.

mod common;

use common::*;
use ferric_core::{ActionProtocol, Message, Role, ToolCall};
use ferric_guard::Workspace;
use ferric_loop::{ReplayedState, RunArgs, StopReason, replay, run};
use ferric_provider::{Completion, MockProvider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};

/// A `NativeTools` tool-call completion with an explicit `input_tokens`
/// (the `common::tool_completion` helper hardcodes 60, too small to ever
/// cross the compaction trigger).
fn tool_completion_with_tokens(
    id: &str,
    name: &str,
    args: serde_json::Value,
    input_tokens: u32,
) -> Completion {
    Completion {
        message: Message {
            role: Role::Assistant,
            text: None,
            tool_calls: vec![ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                args,
            }],
            tool_call_id: None,
            media: Vec::new(),
        },
        input_tokens: Some(input_tokens),
        output_tokens: Some(15),
        truncated: false,
    }
}

/// Like `tool_completion_with_tokens`, but the assistant message ALSO
/// carries prose `text` alongside the tool call (a `NativeTools` completion
/// can do this — see `replay_preserves_native_text_alongside_tool_calls`,
/// sprint 40's own C-005 fix). `PromptAssembled.chars` sums each message's
/// `.text` — a tool call's `args` never contribute — so this is how a test
/// inflates a turn's `chars` contribution deliberately.
fn tool_completion_with_text_and_tokens(
    id: &str,
    name: &str,
    args: serde_json::Value,
    text: &str,
    input_tokens: u32,
) -> Completion {
    Completion {
        message: Message {
            role: Role::Assistant,
            text: Some(text.to_string()),
            tool_calls: vec![ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                args,
            }],
            tool_call_id: None,
            media: Vec::new(),
        },
        input_tokens: Some(input_tokens),
        output_tokens: Some(15),
        truncated: false,
    }
}

fn prompt_assembled_for_turn(records: &[ferric_trace::TraceRecord], turn: u32) -> (u32, u64) {
    records
        .iter()
        .find_map(|r| match &r.event {
            ParsedEvent::Known(Event::PromptAssembled {
                turn: t,
                message_count,
                chars,
                ..
            }) if *t == turn => Some((*message_count, *chars)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no PromptAssembled for turn {turn}"))
}

/// T-4003: enough turns with a large-output tool cross the trigger fraction
/// (85% of `nano_policy()`'s 2800-token budget); the fold happens at the top
/// of the NEXT turn, so THAT turn's own `PromptAssembled` already reflects
/// the shrunk history — both `message_count` AND `chars` (plan-critic C-010:
/// message count alone is a proxy, chars is what actually matters).
#[test]
fn compaction_triggers_and_shrinks_next_prompt_assembled() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "compact-1").unwrap();

    // `PromptAssembled.chars` sums each message's `.text` — a tool call's
    // `args` never contribute (they live in `ToolCall.args`, not `.text`).
    // Give turns 0/1's assistant messages bulky PROSE text (alongside their
    // tool calls) so folding them away produces a measurable `chars` drop.
    let long_text_a = "a".repeat(500);
    let long_text_b = "b".repeat(500);
    let provider = MockProvider::new(vec![
        tool_completion_with_text_and_tokens(
            "tc-0",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "a"}),
            &long_text_a,
            100,
        ),
        tool_completion_with_text_and_tokens(
            "tc-1",
            "write_file",
            serde_json::json!({"path": "b.txt", "content": "y"}),
            &long_text_b,
            100,
        ),
        tool_completion_with_tokens(
            "tc-2",
            "write_file",
            serde_json::json!({"path": "c.txt", "content": "z"}),
            100,
        ),
        // Turn 3's completion reports high input_tokens — crosses 85% of
        // 2800 (2380) — checked at the TOP of turn 4, before turn 4's
        // request is assembled.
        tool_completion_with_tokens(
            "tc-3",
            "write_file",
            serde_json::json!({"path": "d.txt", "content": "w"}),
            2500,
        ),
        // The compaction summarizer's own completion fires FIRST (top of
        // turn 4, before turn 4's own request is built) — must precede
        // turn 4's own completion in script order.
        text_completion("wrote a.txt and b.txt"),
        tool_completion(vec![(
            "tc-4",
            ferric_loop::TASK_COMPLETE,
            serde_json::json!({"summary": "wrote 4 files"}),
        )]),
    ]);
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
        Some("write four files"),
    ))
    .unwrap();
    assert_eq!(outcome.stop, StopReason::TaskComplete);

    let records: Vec<_> = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let history_compacted_count = records
        .iter()
        .filter(|r| matches!(&r.event, ParsedEvent::Known(Event::HistoryCompacted { .. })))
        .count();
    assert_eq!(history_compacted_count, 1, "exactly one fold triggered");

    let (turn3_count, turn3_chars) = prompt_assembled_for_turn(&records, 3);
    let (turn4_count, turn4_chars) = prompt_assembled_for_turn(&records, 4);
    assert!(
        turn4_count < turn3_count,
        "turn 4's message_count ({turn4_count}) should be smaller than turn 3's ({turn3_count}) after the fold"
    );
    assert!(
        turn4_chars < turn3_chars,
        "turn 4's chars ({turn4_chars}) should be smaller than turn 3's ({turn3_chars}) after the fold \
         (the actual token-budget-relevant metric, not just message_count)"
    );
}

/// Plan-critic C-007: a direct regression test for the load-bearing ordering
/// invariant itself (not just its downstream message-count effect) — proves
/// `TurnStart{turn: N}` appears strictly before `HistoryCompacted`, which
/// appears strictly before `TurnEnd{turn: N}`, for the triggering turn.
#[test]
fn history_compacted_traced_after_triggering_turn_start() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "compact-2").unwrap();

    let provider = MockProvider::new(vec![
        tool_completion_with_tokens(
            "tc-0",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "a"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-1",
            "write_file",
            serde_json::json!({"path": "b.txt", "content": "b"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-2",
            "write_file",
            serde_json::json!({"path": "c.txt", "content": "c"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-3",
            "write_file",
            serde_json::json!({"path": "d.txt", "content": "d"}),
            2500,
        ),
        text_completion("summary of turns 0-1"),
        tool_completion(vec![(
            "tc-4",
            ferric_loop::TASK_COMPLETE,
            serde_json::json!({"summary": "done"}),
        )]),
    ]);
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
        Some("write four files"),
    ))
    .unwrap();
    assert_eq!(outcome.stop, StopReason::TaskComplete);

    let records: Vec<_> = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let turn_start_4 = records
        .iter()
        .position(|r| matches!(&r.event, ParsedEvent::Known(Event::TurnStart { turn: 4 })))
        .expect("TurnStart{turn:4}");
    let history_compacted = records
        .iter()
        .position(|r| matches!(&r.event, ParsedEvent::Known(Event::HistoryCompacted { .. })))
        .expect("a HistoryCompacted event");
    let turn_end_4 = records
        .iter()
        .position(|r| matches!(&r.event, ParsedEvent::Known(Event::TurnEnd { turn: 4, .. })))
        .expect("TurnEnd{turn:4}");

    assert!(
        turn_start_4 < history_compacted,
        "TurnStart(4) must precede HistoryCompacted"
    );
    assert!(
        history_compacted < turn_end_4,
        "HistoryCompacted must precede TurnEnd(4)"
    );
}

/// A resumed session's compactor treats the ENTIRE replayed history as its
/// protected head — only NEW turns generated after resuming are foldable (a
/// deliberate v1 scope limit, ADR-050).
#[test]
fn resume_only_folds_new_post_resume_turns() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "compact-resume-1").unwrap();

    let replayed = ReplayedState {
        messages: vec![
            Message::system("You are Ferric."),
            Message::user_with_media("do the task", Vec::new()),
            Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "tc-old".to_string(),
                    name: "write_file".to_string(),
                    args: serde_json::json!({"path": "old.txt", "content": "old"}),
                }],
                tool_call_id: None,
                media: Vec::new(),
            },
            Message::tool_result("tc-old", "wrote old.txt"),
        ],
        turns: 1,
        last_text: None,
        protocol: ActionProtocol::NativeTools,
        source_session: "orig".to_string(),
    };
    let replayed_prefix = replayed.messages.clone();

    let provider = MockProvider::new(vec![
        tool_completion_with_tokens(
            "tc-1",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "a"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-2",
            "write_file",
            serde_json::json!({"path": "b.txt", "content": "b"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-3",
            "write_file",
            serde_json::json!({"path": "c.txt", "content": "c"}),
            2500,
        ),
        text_completion("summary of the new turns"),
        tool_completion(vec![(
            "tc-4",
            ferric_loop::TASK_COMPLETE,
            serde_json::json!({"summary": "done"}),
        )]),
    ]);
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
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(replayed),
            hooks: None,
        },
        &mut sink,
        None,
    ))
    .unwrap();
    assert_eq!(outcome.stop, StopReason::TaskComplete);

    // The last request the provider saw (the terminator turn) must still
    // carry the ENTIRE replayed prefix, byte-identical — it was never
    // eligible for folding.
    let requests = provider.requests();
    let last_request = requests.last().unwrap();
    assert_eq!(
        &last_request.messages[..replayed_prefix.len()],
        &replayed_prefix[..]
    );
}

/// Plan-critic C-006: proves the absolute-turn-number redesign actually
/// resolves the resume+compaction numbering concern — a resumed session
/// (nonzero starting `turns`) that folds NEW turns must trace
/// `HistoryCompacted.through_turn` using the SAME absolute numbers the
/// loop's own `turns` counter assigned, not a reset-to-0 scheme.
#[test]
fn resume_seeds_compactor_numbering_consistently() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "compact-resume-2").unwrap();

    // Resume with turns already at 7 (as if 7 turns happened in the
    // original session).
    let replayed = ReplayedState {
        messages: vec![
            Message::system("You are Ferric."),
            Message::user_with_media("do the task", Vec::new()),
        ],
        turns: 7,
        last_text: None,
        protocol: ActionProtocol::NativeTools,
        source_session: "orig".to_string(),
    };

    let provider = MockProvider::new(vec![
        tool_completion_with_tokens(
            "tc-1",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "a"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-2",
            "write_file",
            serde_json::json!({"path": "b.txt", "content": "b"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-3",
            "write_file",
            serde_json::json!({"path": "c.txt", "content": "c"}),
            2500,
        ),
        text_completion("summary of turns 7-8"),
        tool_completion(vec![(
            "tc-4",
            ferric_loop::TASK_COMPLETE,
            serde_json::json!({"summary": "done"}),
        )]),
    ]);
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
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: Some(replayed),
            hooks: None,
        },
        &mut sink,
        None,
    ))
    .unwrap();
    assert_eq!(outcome.stop, StopReason::TaskComplete);

    let records: Vec<_> = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let through_turn = records
        .iter()
        .find_map(|r| match &r.event {
            ParsedEvent::Known(Event::HistoryCompacted { through_turn, .. }) => Some(*through_turn),
            _ => None,
        })
        .expect("a HistoryCompacted event");
    // Loop turns after resuming start at 7 (turns: 7,8,9,10,...). Only turn 7
    // is folded (completed=[7,8,9], KEEP_LAST_TURNS=2 preserves 8,9) — the
    // exact expected value, not just "at least 7" (test-critic C-001: a loose
    // `>=` bound wouldn't catch an off-by-one shift to 8/9/10).
    assert_eq!(
        through_turn, 7,
        "through_turn must use the loop's own absolute numbering, not reset to 0"
    );
}

/// T-4004's strongest test (mirrors sprint 39's C-010
/// `real_run_then_replay_then_resume_reaches_task_complete`): a REAL `run()`
/// call triggers a REAL compaction, its REAL trace file is truncated
/// (simulating a kill), `replay()`d, and the resulting `ReplayedState`'s
/// history is smaller than the full pre-compaction history would have been —
/// the end-to-end proof the mechanism survives a resume boundary.
#[test]
fn real_run_compact_kill_replay_resume_shrinks_history() {
    let dir1 = tempfile::tempdir().unwrap();
    let workspace1 = Workspace::new(dir1.path()).unwrap();
    let mut registry1 = Registry::new();
    register_builtin_tools(&mut registry1);
    let trace_path = dir1.path().join("trace.jsonl");
    let mut sink1 = JsonlSink::open(&trace_path, "compact-e2e-original").unwrap();

    // Exactly 5 completions: 4 tool turns (the 4th crosses the trigger) plus
    // the compaction summarizer call. Turn 4's OWN completion request (after
    // the fold) then finds the script exhausted — a natural ProviderError
    // stop, appending a SessionEnd line this test drops below to simulate a
    // kill (mirrors sprint 39's C-010 technique exactly).
    let provider1 = MockProvider::new(vec![
        tool_completion_with_tokens(
            "tc-0",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "a"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-1",
            "write_file",
            serde_json::json!({"path": "b.txt", "content": "b"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-2",
            "write_file",
            serde_json::json!({"path": "c.txt", "content": "c"}),
            100,
        ),
        tool_completion_with_tokens(
            "tc-3",
            "write_file",
            serde_json::json!({"path": "d.txt", "content": "d"}),
            2500,
        ),
        text_completion("did a.txt and b.txt"),
    ]);
    let sleeper1 = RecordingSleeper::new();
    let first = futures_executor::block_on(run(
        RunArgs {
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
        Some("write four files"),
    ))
    .unwrap();
    assert_eq!(
        first.stop,
        StopReason::ProviderError,
        "script exhausted on turn 4's own request"
    );

    // Simulate a kill: drop the trailing SessionEnd line (sprint 39's C-010
    // technique) — turn 4 never got a TurnEnd at all, so replay's existing
    // dangling-turn discard correctly drops it; the fold (turns 0-1) and the
    // preserved tail (turns 2-3) survive.
    let content = std::fs::read_to_string(&trace_path).unwrap();
    let mut lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.pop().map(|l| l.contains("session_end")), Some(true));
    std::fs::write(&trace_path, lines.join("\n") + "\n").unwrap();

    let replayed = replay(&trace_path).unwrap();
    // Precise, deterministic shrinkage proof: turns 0-3 each produced 2
    // messages (assistant + tool_result, no guard warnings) = 8, plus the
    // head (2) = 10 if nothing were folded. After folding turns 0-1
    // (KEEP_LAST_TURNS=2 preserves turns 2-3 verbatim): head(2) + summary(1)
    // + turn2(2) + turn3(2) = 7. Turn 4 never got a TurnEnd (dangling,
    // discarded) — contributes nothing.
    assert_eq!(
        replayed.messages.len(),
        7,
        "head(2) + summary(1) + preserved turns 2-3 (2 each) — turns 0-1 folded away, turn 4 dangling"
    );
    assert!(
        replayed.messages.iter().any(|m| m
            .text
            .as_deref()
            .is_some_and(|t| t.starts_with("[compacted history]"))),
        "the resumed history must contain the compaction summary, not the raw folded turns"
    );

    // A second real run, resuming, finishes the task.
    let dir2 = tempfile::tempdir().unwrap();
    let workspace2 = Workspace::new(dir2.path()).unwrap();
    let mut registry2 = Registry::new();
    register_builtin_tools(&mut registry2);
    let trace_path2 = dir2.path().join("trace.jsonl");
    let mut sink2 = JsonlSink::open(&trace_path2, "compact-e2e-continuation").unwrap();
    let provider2 = MockProvider::new(vec![tool_completion(vec![(
        "tc-5",
        ferric_loop::TASK_COMPLETE,
        serde_json::json!({"summary": "wrote five files"}),
    )])]);
    let sleeper2 = RecordingSleeper::new();
    let second = futures_executor::block_on(run(
        RunArgs {
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
}
