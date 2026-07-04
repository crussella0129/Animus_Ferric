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

use ferric_core::{ActionProtocol, FerricError, Message, Role, ToolCall};
use ferric_trace::{Event, ParsedEvent, TraceReader};
use thiserror::Error;

use crate::run::{
    failure_warn_message, no_action_nudge, no_progress_warn_message, repetition_warn_message,
    result_message, truncation_retry_message,
};

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

/// One turn's data, buffered until a later `TurnStart` (or EOF) decides
/// whether it's confirmed complete or dangling.
#[derive(Default)]
struct PendingTurn {
    /// `(text, truncated)` from this turn's `TurnEnd`, once seen.
    turn_end: Option<(Option<String>, bool)>,
    /// `ToolCall` events for this turn, in trace order (includes the
    /// terminator's, per T-3902 — never actually reachable here in practice,
    /// since a turn that truly terminated would have written `SessionEnd`
    /// and been rejected before reconstruction ever starts).
    tool_calls: Vec<ToolCall>,
    /// `(id, name, output)` from this turn's `ToolResult` events, in order.
    tool_results: Vec<(String, String, String)>,
    repetition_warned: bool,
    no_progress_warned: bool,
    failure_warned: bool,
}

impl PendingTurn {
    /// Turn this buffer into the messages `run()` would have pushed for it,
    /// or `None` if it never received a `TurnEnd` at all (defensively treated
    /// the same as a dangling turn).
    fn finalize(self, protocol: ActionProtocol) -> Option<Vec<Message>> {
        let (text, truncated) = self.turn_end?;

        // Truncation handling is ConstrainedJson-only (run.rs mirrors this
        // exact gate) — other protocols ignore the flag and proceed normally.
        if truncated && protocol == ActionProtocol::ConstrainedJson {
            return Some(vec![truncation_retry_message()]);
        }

        let assistant = match protocol {
            ActionProtocol::NativeTools => Message {
                role: Role::Assistant,
                text,
                tool_calls: self.tool_calls.clone(),
                tool_call_id: None,
                media: Vec::new(),
            },
            ActionProtocol::ConstrainedJson | ActionProtocol::TextXml => {
                Message::assistant(text.unwrap_or_default())
            }
        };

        if self.tool_calls.is_empty() {
            // No traced ToolCall events this turn: run() would have hit the
            // no-action nudge and `continue`d — no guards, no dispatch.
            return Some(vec![assistant, Message::user(no_action_nudge(protocol))]);
        }

        let mut out = vec![assistant];
        let names: Vec<&str> = self.tool_calls.iter().map(|c| c.name.as_str()).collect();
        if self.repetition_warned {
            out.push(repetition_warn_message(&names));
        }
        if self.no_progress_warned {
            out.push(no_progress_warn_message(&names));
        }
        for (id, name, output) in &self.tool_results {
            out.push(result_message(protocol, id, name, output));
        }
        if self.failure_warned {
            out.push(failure_warn_message());
        }
        Some(out)
    }
}

/// Reconstruct a `ReplayedState` from the trace at `path`.
pub fn replay(path: &Path) -> Result<ReplayedState, ReplayError> {
    // Pass 1: any SessionEnd at all means this session isn't "interrupted."
    for record in TraceReader::open(path)? {
        if let ParsedEvent::Known(Event::SessionEnd { reason }) = record?.event {
            return Err(ReplayError::AlreadyStopped(reason));
        }
    }

    // Pass 2: reconstruct.
    let mut source_session: Option<String> = None;
    let mut protocol: Option<ActionProtocol> = None;
    let mut messages: Vec<Message> = Vec::new();
    let mut turns: u32 = 0;
    let mut last_text: Option<String> = None;
    let mut pending: Option<PendingTurn> = None;
    let mut session_prompt_seen = false;

    // Commits the currently-open pending turn (a later TurnStart confirms it
    // finished dispatching) and starts a fresh one.
    macro_rules! commit_and_reset {
        () => {
            if let Some(p) = pending.take() {
                let proto = protocol.ok_or(ReplayError::MissingSessionPrompt)?;
                if let Some(msgs) = p.finalize(proto) {
                    // Track NativeTools' "best-effort final text" the same way
                    // run() does: last non-empty text across committed turns.
                    if proto == ActionProtocol::NativeTools
                        && let Some(assistant) = msgs.first()
                        && let Some(t) = &assistant.text
                        && !t.is_empty()
                    {
                        last_text = Some(t.clone());
                    }
                    messages.extend(msgs);
                    turns += 1;
                }
            }
        };
    }

    for record in TraceReader::open(path)? {
        let record = record?;
        if source_session.is_none() {
            source_session = Some(record.session.clone());
        }
        match record.event {
            ParsedEvent::Known(Event::PolicySelected { protocol: p, .. }) => {
                protocol = Some(p);
            }
            ParsedEvent::Known(Event::SessionPrompt {
                system,
                user,
                media,
            }) => {
                messages.push(Message::system(system));
                messages.push(Message::user_with_media(user, media));
                session_prompt_seen = true;
            }
            ParsedEvent::Known(Event::TurnStart { .. }) => {
                commit_and_reset!();
                pending = Some(PendingTurn::default());
            }
            ParsedEvent::Known(Event::TurnEnd {
                text, truncated, ..
            }) => {
                if let Some(p) = &mut pending {
                    p.turn_end = Some((text, truncated));
                }
            }
            ParsedEvent::Known(Event::ToolCall { id, name, args }) => {
                if let Some(p) = &mut pending {
                    p.tool_calls.push(ToolCall { id, name, args });
                }
            }
            ParsedEvent::Known(Event::ToolResult {
                id, name, output, ..
            }) => {
                if let Some(p) = &mut pending {
                    p.tool_results.push((id, name, output));
                }
            }
            ParsedEvent::Known(Event::RepetitionGuard { action }) if action == "warned" => {
                if let Some(p) = &mut pending {
                    p.repetition_warned = true;
                }
            }
            ParsedEvent::Known(Event::NoProgressGuard { action }) if action == "warned" => {
                if let Some(p) = &mut pending {
                    p.no_progress_warned = true;
                }
            }
            ParsedEvent::Known(Event::FailureGuard { action }) if action == "warned" => {
                if let Some(p) = &mut pending {
                    p.failure_warned = true;
                }
            }
            _ => {}
        }
    }
    // EOF: the still-open `pending` turn (if any) never saw a confirming
    // next `TurnStart` — dangling, discarded, never committed or counted.

    if !session_prompt_seen {
        return Err(ReplayError::MissingSessionPrompt);
    }
    let protocol = protocol.ok_or(ReplayError::MissingSessionPrompt)?;
    let source_session = source_session.ok_or(ReplayError::MissingSessionPrompt)?;

    Ok(ReplayedState {
        messages,
        turns,
        last_text,
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
            max_tools: 6,
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
}
