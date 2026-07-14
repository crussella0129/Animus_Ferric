//! Reconstruct an interrupted session's in-memory turn-loop state from its
//! JSONL trace (sprint 39, ADR-049), so `run()` can continue it with more
//! turns instead of starting the task over from scratch.
//!
//! Scope: **resuming an interrupted, still-incomplete task only** — a trace
//! that already reached any `StopReason` (clean or not) isn't "interrupted";
//! `replay` refuses those (`ReplayError::AlreadyStopped`). This is the ADR-011
//! boundary: not a chat-continuation mechanism.
//!
//! # Why this isn't a simple single-pass walk
//! `TurnEnd` is written *before* dispatch, not after (see `run.rs`) — so
//! "this turn has a `TurnEnd`" does NOT mean its dispatch (tool calls,
//! guard checks, results) finished; a crash mid-dispatch leaves a `TurnEnd`
//! on disk with an incomplete tail. The only reliable "this turn's dispatch
//! fully ran" signal is reaching the *next* `TurnStart` (or, for the last
//! turn in a surviving — i.e. not-yet-`SessionEnd`ed — trace, never: it's
//! discarded). So each turn is buffered and only committed to the returned
//! `messages` once a later `TurnStart` confirms it's done.

use std::path::Path;

use ferric_core::{ActionProtocol, FerricError, Message};
use ferric_trace::{Event, ParsedEvent, TraceReader};
use thiserror::Error;

use crate::projector::TraceProjector;

/// Everything `run()` needs to seed a continuing turn loop.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayedState {
    pub messages: Vec<Message>,
    pub turns: u32,
    pub last_text: Option<String>,
    pub protocol: ActionProtocol,
    /// The original session's `session` id (not a file path — stable even if
    /// trace files move). Threaded into the continuing session's
    /// `SessionStart.resumed_from`.
    pub source_session: String,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("trace error: {0}")]
    Trace(#[from] FerricError),
    #[error(
        "trace has no SessionPrompt event — not a resumable session (missing, foreign, or pre-sprint-39 trace file)"
    )]
    MissingSessionPrompt,
    #[error("session already ended ({0}) — resume is only for interrupted sessions")]
    AlreadyStopped(String),
}

pub fn replay(path: &Path) -> Result<ReplayedState, ReplayError> {
    // Pass 1: any SessionEnd at all means this session isn't "interrupted."
    for record in TraceReader::open(path)? {
        if let ParsedEvent::Known(Event::SessionEnd { reason }) = record?.event {
            return Err(ReplayError::AlreadyStopped(reason));
        }
    }

    // Pass 2: reconstruct.
    let mut source_session: Option<String> = None;
    let mut projector = TraceProjector::new();

    for record in TraceReader::open(path)? {
        let record = record?;
        if source_session.is_none() {
            source_session = Some(record.session.clone());
        }
        if let ParsedEvent::Known(event) = record.event {
            projector.step(&event);
        }
    }
    // EOF: the still-open `pending` turn (if any) never saw a confirming
    // next `TurnStart` — dangling, discarded, never committed or counted.

    if projector.head_len == 0 {
        return Err(ReplayError::MissingSessionPrompt);
    }
    let protocol = projector.protocol.ok_or(ReplayError::MissingSessionPrompt)?;
    let source_session = source_session.ok_or(ReplayError::MissingSessionPrompt)?;

    Ok(ReplayedState {
        messages: projector.messages,
        turns: projector.turns,
        last_text: projector.last_text,
        protocol,
        source_session,
    })
}

#[cfg(test)]
mod tests {
    //! Co-located (not `tests/`) so these can call the `pub(crate)`
    //! nudge-formatting helpers directly for exact-message comparison —
    //! `tests/` integration tests compile against the crate as an external
    //! dependency and can't see `pub(crate)` items.
    use super::*;
    use crate::projector::*;
    use ferric_trace::JsonlSink;
    use serde_json::json;

    fn write_trace(events: &[Event]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let mut sink = JsonlSink::open(&path, "s-1").unwrap();
        for e in events {
            sink.write_event(e.clone()).unwrap();
        }
        (dir, path)
    }

    fn policy_selected(protocol: ActionProtocol) -> Event {
        Event::PolicySelected {
            tier: ferric_core::Tier::Nano,
            protocol,
            max_turns: 15,
            max_tools: 10,
            prompt_budget_tokens: 2_800,
            max_output_tokens: 512,
        }
    }

    fn session_prompt() -> Event {
        Event::SessionPrompt {
            system: "You are Ferric.".to_string(),
            user: "do the task".to_string(),
            media: Vec::new(),
        }
    }

    fn expected_prefix() -> Vec<Message> {
        vec![
            Message::system("You are Ferric."),
            Message::user_with_media("do the task", Vec::new()),
        ]
    }

    #[test]
    fn replay_reconstructs_a_clean_constrained_json_session() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::ConstrainedJson),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: Some(r#"{"tool":"read_file","args":{"path":"a.txt"}}"#.to_string()),
                tool_call_count: 0,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::ToolCall {
                id: "g-0-0".to_string(),
                name: "read_file".to_string(),
                args: json!({"path": "a.txt"}),
            },
            Event::ToolResult {
                id: "g-0-0".to_string(),
                name: "read_file".to_string(),
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
            Event::TurnEnd {
                turn: 1,
                text: Some(r#"{"tool":"task_complete","args":{"summary":"done"}}"#.to_string()),
                tool_call_count: 0,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::ToolCall {
                id: "g-1-0".to_string(),
                name: "task_complete".to_string(),
                args: json!({"summary": "done"}),
            },
            // Confirms turn 1's dispatch fully ran (no crash mid-turn).
            Event::TurnStart { turn: 2 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        let mut expected = expected_prefix();
        expected.push(Message::assistant(
            r#"{"tool":"read_file","args":{"path":"a.txt"}}"#,
        ));
        expected.push(Message::user("[tool_result for read_file] contents"));
        expected.push(Message::assistant(
            r#"{"tool":"task_complete","args":{"summary":"done"}}"#,
        ));
        assert_eq!(replayed.messages, expected);
        assert_eq!(replayed.turns, 2);
        assert_eq!(replayed.protocol, ActionProtocol::ConstrainedJson);
        assert_eq!(replayed.source_session, "s-1");
    }

    #[test]
    fn replay_preserves_native_multi_tool_call_order() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 2,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::ToolCall {
                id: "tc-0".to_string(),
                name: "tool_a".to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id: "tc-0".to_string(),
                name: "tool_a".to_string(),
                output: "a-out".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::ToolCall {
                id: "tc-1".to_string(),
                name: "tool_b".to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id: "tc-1".to_string(),
                name: "tool_b".to_string(),
                output: "b-out".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        let assistant = &replayed.messages[2];
        assert_eq!(
            assistant
                .tool_calls
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["tool_a", "tool_b"]
        );
    }

    /// Test-critic C-005: a `NativeTools` completion can carry BOTH prose
    /// text and tool calls in the same message (`run()` pushes
    /// `completion.message` verbatim) — every other test here exercises the
    /// two fields in isolation. Prove both land in the reconstructed message.
    #[test]
    fn replay_preserves_native_text_alongside_tool_calls() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: Some("thinking...".to_string()),
                tool_call_count: 1,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::ToolCall {
                id: "tc-0".to_string(),
                name: "tool_a".to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id: "tc-0".to_string(),
                name: "tool_a".to_string(),
                output: "a-out".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        let assistant = &replayed.messages[2];
        assert_eq!(assistant.text.as_deref(), Some("thinking..."));
        assert_eq!(
            assistant
                .tool_calls
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["tool_a"]
        );
    }

    /// Test-critic C-003: the terminator mixed into the MIDDLE of a
    /// multi-tool-call turn (not last) must preserve exact original order in
    /// the reconstructed assistant message.
    #[test]
    fn replay_preserves_terminator_position_mid_turn() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 3,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::ToolCall {
                id: "tc-0".to_string(),
                name: "tool_a".to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id: "tc-0".to_string(),
                name: "tool_a".to_string(),
                output: "a-out".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::ToolCall {
                id: "tc-1".to_string(),
                name: "task_complete".to_string(),
                args: json!({"summary": "done"}),
            },
            Event::ToolCall {
                id: "tc-2".to_string(),
                name: "tool_b".to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id: "tc-2".to_string(),
                name: "tool_b".to_string(),
                output: "b-out".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        let assistant = &replayed.messages[2];
        assert_eq!(
            assistant
                .tool_calls
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["tool_a", "task_complete", "tool_b"],
            "terminator position must match original order, not be moved to the end"
        );
    }

    #[test]
    fn replay_reconstructs_repetition_guard_nudge() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::RepetitionGuard {
                action: "warned".to_string(),
            },
            Event::ToolCall {
                id: "tc-0".to_string(),
                name: "read_file".to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id: "tc-0".to_string(),
                name: "read_file".to_string(),
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(
            replayed.messages[3],
            repetition_warn_message(&["read_file"])
        );
    }

    #[test]
    fn replay_reconstructs_no_progress_guard_nudge() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::NoProgressGuard {
                action: "warned".to_string(),
            },
            Event::ToolCall {
                id: "tc-0".to_string(),
                name: "read_file".to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id: "tc-0".to_string(),
                name: "read_file".to_string(),
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(
            replayed.messages[3],
            no_progress_warn_message(&["read_file"])
        );
        // Not collapsed into the repetition guard's different wording (C-007).
        assert_ne!(
            replayed.messages[3],
            repetition_warn_message(&["read_file"])
        );
    }

    /// Test-critic C-005: the exact `TextXml` parse-error text isn't traced
    /// (an accepted approximation) — replay must still produce a valid,
    /// non-empty nudge (the generic template), never panic.
    #[test]
    fn replay_reconstructs_xml_parse_error_nudge_falls_back_to_generic() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::TextXml),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: Some("not a valid tool_call tag".to_string()),
                tool_call_count: 0,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            // No ToolCall events: the parse failed, mirroring run()'s real
            // zero-actions path for a bad XML completion.
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(
            replayed.messages[3],
            Message::user(no_action_nudge(ActionProtocol::TextXml))
        );
    }

    #[test]
    fn replay_reconstructs_truncated_turn_retry() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::ConstrainedJson),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: Some(r#"{"tool":"write_file","args":{"path":"a.tx"#.to_string()),
                tool_call_count: 0,
                input_tokens: Some(50),
                output_tokens: Some(512),
                truncated: true,
            },
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        let mut expected = expected_prefix();
        expected.push(truncation_retry_message());
        assert_eq!(replayed.messages, expected);
        assert_eq!(replayed.turns, 1);
    }

    /// Test-critic C-001: the realistic shape of a killed process — a
    /// `TurnStart` with no matching `TurnEnd` at all.
    #[test]
    fn replay_discards_a_dangling_mid_turn() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            // Crash: no TurnEnd, no further events at all.
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(replayed.messages, expected_prefix());
        assert_eq!(replayed.turns, 0);
    }

    /// A stricter variant of C-001: even a turn WITH a `TurnEnd` (and partial
    /// dispatch) is discarded if no LATER `TurnStart` confirms dispatch fully
    /// ran — `TurnEnd` is written *before* dispatch in `run()`, so its mere
    /// presence doesn't prove the turn finished.
    #[test]
    fn replay_discards_a_turn_end_with_no_confirming_next_turn_start() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::ToolCall {
                id: "tc-0".to_string(),
                name: "read_file".to_string(),
                args: json!({}),
            },
            // Crash mid-dispatch: no ToolResult, no next TurnStart.
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(replayed.messages, expected_prefix());
        assert_eq!(replayed.turns, 0);
    }

    #[test]
    fn replay_missing_session_prompt_is_an_error() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
        ];
        let (_dir, path) = write_trace(&events);
        assert!(matches!(
            replay(&path),
            Err(ReplayError::MissingSessionPrompt)
        ));
    }

    #[test]
    fn replay_already_stopped_is_an_error() {
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: Some("all done".to_string()),
                tool_call_count: 0,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::SessionEnd {
                reason: "final_text".to_string(),
            },
        ];
        let (_dir, path) = write_trace(&events);
        match replay(&path) {
            Err(ReplayError::AlreadyStopped(reason)) => assert_eq!(reason, "final_text"),
            other => panic!("expected AlreadyStopped, got {other:?}"),
        }
    }

    fn turn_events(turn: u32, tool: &str, output: &str) -> Vec<Event> {
        let id = format!("tc-{turn}");
        vec![
            Event::TurnStart { turn },
            Event::TurnEnd {
                turn,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(50),
                output_tokens: Some(10),
                truncated: false,
            },
            Event::ToolCall {
                id: id.clone(),
                name: tool.to_string(),
                args: json!({}),
            },
            Event::ToolResult {
                id,
                name: tool.to_string(),
                output: output.to_string(),
                is_error: false,
                duration_ms: 1,
            },
        ]
    }

    /// Sprint 40 (ADR-050): a single `HistoryCompacted` fold reconstructs
    /// `messages` with the folded turns dropped, the summary inserted after
    /// the head, and the preserved tail byte-identical/ordered. Places
    /// `HistoryCompacted` in its REAL shape (right after the triggering
    /// turn's own `TurnStart`, before its `TurnEnd`) for fixture realism,
    /// matching `run.rs`'s actual output — but this positioning is NOT
    /// load-bearing for what THIS test verifies: the match arm reconstructs
    /// from `committed_turn_starts` alone (advanced only via `TurnStart`),
    /// never touching `pending`, so the result is identical wherever the
    /// event sits within turn 5's own span. The byte-order invariant itself
    /// (test-critic C-002) is verified end-to-end by
    /// `history_compacted_traced_after_triggering_turn_start` in
    /// `compaction_tests.rs`, against a REAL `run()`-produced trace.
    #[test]
    fn replay_applies_one_history_compaction() {
        let mut events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
        ];
        events.extend(turn_events(0, "tool_a", "a-out"));
        events.extend(turn_events(1, "tool_b", "b-out"));
        events.extend(turn_events(2, "tool_c", "c-out"));
        events.extend(turn_events(3, "tool_d", "d-out"));
        events.extend(turn_events(4, "tool_e", "e-out"));
        // Turn 5 triggers a fold covering turns 0-2 (KEEP_LAST_TURNS=2 shape:
        // turns 3,4 survive verbatim).
        events.push(Event::TurnStart { turn: 5 });
        events.push(Event::HistoryCompacted {
            through_turn: 2,
            dropped_turns: 3,
            summary: "did a, b, c".to_string(),
        });
        events.push(Event::TurnEnd {
            turn: 5,
            text: None,
            tool_call_count: 1,
            input_tokens: Some(50),
            output_tokens: Some(10),
            truncated: false,
        });
        events.push(Event::ToolCall {
            id: "tc-5".to_string(),
            name: "tool_f".to_string(),
            args: json!({}),
        });
        events.push(Event::ToolResult {
            id: "tc-5".to_string(),
            name: "tool_f".to_string(),
            output: "f-out".to_string(),
            is_error: false,
            duration_ms: 1,
        });
        events.push(Event::TurnStart { turn: 6 }); // confirms turn 5 committed

        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(replayed.messages.len(), 9);
        assert_eq!(
            replayed.messages[2].text.as_deref(),
            Some("[compacted history] did a, b, c")
        );
        assert_eq!(replayed.messages[3].tool_calls[0].name, "tool_d");
        assert_eq!(replayed.messages[5].tool_calls[0].name, "tool_e");
        assert_eq!(replayed.messages[7].tool_calls[0].name, "tool_f");
        assert_eq!(replayed.turns, 6, "total commits, folded or not");
    }

    /// Sprint 40: a SECOND `HistoryCompacted` later in the same trace folds
    /// the surviving tail from the first fold together with newly-eligible
    /// turns into ONE new summary — the old summary message must not linger
    /// alongside the new one.
    #[test]
    fn replay_applies_two_sequential_history_compactions() {
        let mut events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
        ];
        events.extend(turn_events(0, "tool_a", "a-out"));
        events.extend(turn_events(1, "tool_b", "b-out"));
        events.extend(turn_events(2, "tool_c", "c-out"));
        events.extend(turn_events(3, "tool_d", "d-out"));
        events.extend(turn_events(4, "tool_e", "e-out"));
        events.push(Event::TurnStart { turn: 5 });
        events.push(Event::HistoryCompacted {
            through_turn: 2,
            dropped_turns: 3,
            summary: "did a, b, c".to_string(),
        });
        events.push(Event::TurnEnd {
            turn: 5,
            text: None,
            tool_call_count: 1,
            input_tokens: Some(50),
            output_tokens: Some(10),
            truncated: false,
        });
        events.push(Event::ToolCall {
            id: "tc-5".to_string(),
            name: "tool_f".to_string(),
            args: json!({}),
        });
        events.push(Event::ToolResult {
            id: "tc-5".to_string(),
            name: "tool_f".to_string(),
            output: "f-out".to_string(),
            is_error: false,
            duration_ms: 1,
        });
        events.extend(turn_events(6, "tool_g", "g-out"));
        events.extend(turn_events(7, "tool_h", "h-out"));
        // Turn 8 triggers a SECOND fold covering turns 3-5 (the surviving
        // tail from fold 1, plus the new turn 5) — turns 6,7 now survive.
        events.push(Event::TurnStart { turn: 8 });
        events.push(Event::HistoryCompacted {
            through_turn: 5,
            dropped_turns: 3,
            summary: "did d, e, f".to_string(),
        });
        events.push(Event::TurnEnd {
            turn: 8,
            text: None,
            tool_call_count: 1,
            input_tokens: Some(50),
            output_tokens: Some(10),
            truncated: false,
        });
        events.push(Event::ToolCall {
            id: "tc-8".to_string(),
            name: "tool_i".to_string(),
            args: json!({}),
        });
        events.push(Event::ToolResult {
            id: "tc-8".to_string(),
            name: "tool_i".to_string(),
            output: "i-out".to_string(),
            is_error: false,
            duration_ms: 1,
        });
        events.push(Event::TurnStart { turn: 9 }); // confirms turn 8 committed

        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(replayed.messages.len(), 9);
        assert_eq!(
            replayed.messages[2].text.as_deref(),
            Some("[compacted history] did d, e, f"),
            "the LATEST fold's summary, not the first"
        );
        assert!(
            !replayed
                .messages
                .iter()
                .any(|m| m.text.as_deref() == Some("[compacted history] did a, b, c")),
            "the first fold's summary must not linger alongside the second"
        );
        assert_eq!(replayed.messages[3].tool_calls[0].name, "tool_g");
        assert_eq!(replayed.messages[5].tool_calls[0].name, "tool_h");
        assert_eq!(replayed.messages[7].tool_calls[0].name, "tool_i");
        assert_eq!(replayed.turns, 9, "total commits across both folds");
    }
}
