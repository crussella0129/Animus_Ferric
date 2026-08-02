//! Reconstruct a still-incomplete session from its JSONL source of truth.
//!
//! Modern traces use `ActionsProposed` and `TurnCommitted` as an explicit
//! write-ahead protocol. A crash before dispatch is retryable, a tail whose
//! calls all have durable results is recoverable, and a dispatched call with
//! no result is reported as ambiguous instead of being silently repeated.
//! Pre-recovery traces retain their next-`TurnStart` commit semantics.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferric_core::{ActionProtocol, FerricError, Message, UserInputRequest};
use ferric_trace::{Event, GuardTurn, ParsedEvent, RECOVERY_CHECKPOINT_VERSION, TraceReader};
use thiserror::Error;

use crate::projector::TraceProjector;
use crate::trace_structure::TraceStructure;

/// Everything `run()` needs to seed a continuing turn loop.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayedState {
    pub messages: Vec<Message>,
    /// Total committed turns retained for caller compatibility.
    pub turns: u32,
    /// Absolute id to assign to the next turn. A resumed run receives a fresh
    /// per-run budget; this value is not itself a budget counter.
    pub next_turn: u32,
    pub last_text: Option<String>,
    pub protocol: ActionProtocol,
    pub truncation_limit: usize,
    /// The original session's `session` id (not a file path — stable even if
    /// trace files move). Threaded into the continuing session's
    /// `SessionStart.resumed_from`.
    pub source_session: String,
    pub workspace: PathBuf,
    pub head_len: usize,
    pub committed_turn_starts: Vec<(u32, usize)>,
    pub guard_history: Vec<GuardTurn>,
    pub nudged_for_no_action: bool,
    pub truncated_once: bool,
    pub last_input_tokens: Option<u32>,
    pub pending_input: Option<UserInputRequest>,
    pub mutation_epoch: u64,
    pub passed_checks: std::collections::BTreeMap<String, u64>,
    /// The intentional incomplete stop, or `None` for an abrupt crash.
    pub pause_reason: Option<String>,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("trace error: {0}")]
    Trace(#[from] FerricError),
    #[error(
        "trace has no SessionPrompt event — not a resumable session (missing, foreign, or pre-sprint-39 trace file)"
    )]
    MissingSessionPrompt,
    #[error("session completed successfully ({0}) and cannot be resumed")]
    AlreadyStopped(String),
    #[error("trace contains an event this binary does not understand at sequence {seq}")]
    UnknownEvent { seq: u64 },
    #[error("trace mixes session ids {expected:?} and {actual:?}")]
    MixedSessions { expected: String, actual: String },
    #[error("trace has no SessionStart workspace")]
    MissingWorkspace,
    #[error("unsupported recovery checkpoint version {0}")]
    UnsupportedCheckpoint(u32),
    #[error("invalid recovery trace: {0}")]
    InvalidStructure(String),
    #[error(
        "turn {turn} has dispatched calls with no durable result ({calls:?}); workspace state is ambiguous"
    )]
    AmbiguousTail { turn: u32, calls: Vec<String> },
    #[error("recorded protocol {recorded:?} does not match requested protocol {requested:?}")]
    ProtocolMismatch {
        recorded: ActionProtocol,
        requested: ActionProtocol,
    },
    #[error("recorded workspace {recorded} does not match requested workspace {requested}")]
    WorkspaceMismatch { recorded: String, requested: String },
}

pub fn replay(path: &Path) -> Result<ReplayedState, ReplayError> {
    let mut source_session: Option<String> = None;
    let mut workspace: Option<PathBuf> = None;
    let mut projector = TraceProjector::new();
    let mut pause_reason: Option<String> = None;
    let mut end_reason: Option<String> = None;
    let mut saw_session_prompt = false;
    let mut saw_state_base = false;
    let mut saw_modern_turn = false;
    let mut structure = TraceStructure::new();

    for record in TraceReader::open(path)? {
        let record = record?;
        match &source_session {
            None => source_session = Some(record.session.clone()),
            Some(expected) if expected != &record.session => {
                return Err(ReplayError::MixedSessions {
                    expected: expected.clone(),
                    actual: record.session,
                });
            }
            Some(_) => {}
        }

        let event = match record.event {
            ParsedEvent::Known(event) => event,
            ParsedEvent::Unknown(_) => return Err(ReplayError::UnknownEvent { seq: record.seq }),
        };

        structure
            .observe(&event)
            .map_err(ReplayError::InvalidStructure)?;

        match &event {
            Event::SessionStart {
                workspace: recorded,
                ..
            } => {
                if workspace.replace(PathBuf::from(recorded)).is_some() {
                    return Err(ReplayError::InvalidStructure(
                        "more than one SessionStart event".to_string(),
                    ));
                }
            }
            Event::SessionPrompt { .. } => {
                if saw_state_base {
                    return Err(ReplayError::InvalidStructure(
                        "more than one initial state base".to_string(),
                    ));
                }
                saw_session_prompt = true;
                saw_state_base = true;
            }
            Event::RecoveryCheckpoint { state } => {
                if state.version != RECOVERY_CHECKPOINT_VERSION {
                    return Err(ReplayError::UnsupportedCheckpoint(state.version));
                }
                validate_checkpoint(state)?;
                // A checkpoint before any prompt/turn is the initial base of a
                // resumed trace. Later checkpoints are durable state anchors.
                if !saw_state_base {
                    saw_state_base = true;
                }
            }
            Event::ActionsProposed { .. } | Event::TurnCommitted { .. } => {
                saw_modern_turn = true;
            }
            Event::SessionPaused { reason } => {
                if pause_reason.as_deref().is_some_and(|prior| prior != reason) {
                    return Err(ReplayError::InvalidStructure(
                        "conflicting SessionPaused reasons".to_string(),
                    ));
                }
                pause_reason = Some(reason.clone());
            }
            Event::SessionEnd { reason } => {
                if is_success_reason(reason) {
                    return Err(ReplayError::AlreadyStopped(reason.clone()));
                }
                if !is_resumable_reason(reason) {
                    return Err(ReplayError::InvalidStructure(format!(
                        "unknown SessionEnd reason {reason:?}"
                    )));
                }
                end_reason = Some(reason.clone());
            }
            _ => {}
        }

        projector.step(&event);
    }

    structure.finish().map_err(ReplayError::InvalidStructure)?;

    // A durable successful commit is terminal even if the process crashed in
    // the tiny window before writing SessionEnd. Never resume completed work.
    if end_reason.is_none()
        && let Some(reason) = structure.committed_terminal_reason()
        && is_success_reason(reason)
    {
        return Err(ReplayError::AlreadyStopped(reason.to_string()));
    }

    if let (Some(paused), Some(ended)) = (&pause_reason, &end_reason)
        && paused != ended
    {
        return Err(ReplayError::InvalidStructure(format!(
            "SessionPaused reason {paused:?} differs from SessionEnd {ended:?}"
        )));
    }
    if pause_reason.is_none() {
        pause_reason =
            end_reason.or_else(|| structure.unclosed_terminal_reason().map(str::to_string));
    }

    // A modern EOF tail can be recovered when every call that crossed the
    // dispatch boundary has a result. Missing results are deliberately not
    // guessed: the call may have mutated the workspace before the crash.
    if let Some(pending) = projector.pending.as_ref()
        && pending.actions_proposed.is_some()
    {
        let result_ids: BTreeSet<&str> = pending
            .tool_results
            .iter()
            .map(|(id, _, _, _)| id.as_str())
            .collect();
        let unresolved: Vec<String> = pending
            .tool_calls
            .iter()
            .filter(|call| !result_ids.contains(call.id.as_str()) && !is_control_call(&call.name))
            .map(|call| format!("{}:{}", call.id, call.name))
            .collect();
        if !unresolved.is_empty() {
            return Err(ReplayError::AmbiguousTail {
                turn: pending.turn,
                calls: unresolved,
            });
        }

        let proposed_len = pending.actions_proposed.as_ref().map_or(0, Vec::len);
        if !pending.tool_results.is_empty() && pending.tool_calls.len() != proposed_len {
            return Err(ReplayError::AmbiguousTail {
                turn: pending.turn,
                calls: vec!["partially dispatched multi-call batch".to_string()],
            });
        }
        if !pending.tool_results.is_empty() {
            let dispatched = pending.tool_results.len() as u32;
            let errored = pending
                .tool_results
                .iter()
                .filter(|(_, _, _, is_error)| *is_error)
                .count() as u32;
            projector.commit_pending_with(dispatched, errored, None);
        }
    } else if end_reason_is_legacy_pause(&pause_reason, saw_modern_turn) {
        // A pre-recovery SessionEnd was only written after run() had returned
        // from dispatch, so it remains a valid implicit barrier.
        projector.commit_pending();
    }

    if !saw_state_base || (projector.head_len == 0 && !saw_session_prompt) {
        return Err(ReplayError::MissingSessionPrompt);
    }
    let protocol = projector
        .protocol
        .ok_or(ReplayError::MissingSessionPrompt)?;
    let source_session = source_session.ok_or(ReplayError::MissingSessionPrompt)?;

    Ok(ReplayedState {
        messages: projector.messages,
        turns: projector.turns,
        next_turn: projector.next_turn,
        last_text: projector.last_text,
        protocol,
        truncation_limit: projector.truncation_limit,
        source_session,
        workspace: workspace.ok_or(ReplayError::MissingWorkspace)?,
        head_len: projector.head_len,
        committed_turn_starts: projector.committed_turn_starts,
        guard_history: projector.guard_history,
        nudged_for_no_action: projector.nudged_for_no_action,
        truncated_once: projector.truncated_once,
        last_input_tokens: projector.last_input_tokens,
        pending_input: projector.pending_input,
        mutation_epoch: projector.mutation_epoch,
        passed_checks: projector.passed_checks,
        pause_reason,
    })
}

/// Validate the two pieces of operator-selected execution context that a trace
/// must never silently change across a resume boundary.
pub fn validate_resume_target(
    state: &ReplayedState,
    workspace: &Path,
    protocol: ActionProtocol,
) -> Result<(), ReplayError> {
    if state.protocol != protocol {
        return Err(ReplayError::ProtocolMismatch {
            recorded: state.protocol,
            requested: protocol,
        });
    }

    let recorded =
        std::fs::canonicalize(&state.workspace).map_err(|_| ReplayError::WorkspaceMismatch {
            recorded: state.workspace.display().to_string(),
            requested: workspace.display().to_string(),
        })?;
    let requested =
        std::fs::canonicalize(workspace).map_err(|_| ReplayError::WorkspaceMismatch {
            recorded: state.workspace.display().to_string(),
            requested: workspace.display().to_string(),
        })?;
    if recorded != requested {
        return Err(ReplayError::WorkspaceMismatch {
            recorded: recorded.display().to_string(),
            requested: requested.display().to_string(),
        });
    }
    Ok(())
}

fn validate_checkpoint(state: &ferric_trace::RecoveryCheckpointV1) -> Result<(), ReplayError> {
    if state.head_len > state.messages.len() {
        return Err(ReplayError::InvalidStructure(
            "checkpoint head_len exceeds message count".to_string(),
        ));
    }
    if state
        .committed_turn_starts
        .iter()
        .any(|boundary| boundary.message_index > state.messages.len())
    {
        return Err(ReplayError::InvalidStructure(
            "checkpoint turn boundary exceeds message count".to_string(),
        ));
    }
    if let Some(request) = &state.pending_input {
        request.validate().map_err(|error| {
            ReplayError::InvalidStructure(format!("invalid pending input request: {error}"))
        })?;
    }
    if state
        .passed_checks
        .values()
        .any(|epoch| *epoch > state.mutation_epoch)
    {
        return Err(ReplayError::InvalidStructure(
            "checkpoint contains check evidence from a future mutation epoch".to_string(),
        ));
    }
    Ok(())
}

fn is_control_call(name: &str) -> bool {
    matches!(
        name,
        crate::terminator::TASK_COMPLETE
            | crate::terminator::SUBMIT_PLAN
            | crate::terminator::REQUEST_USER_INPUT
    )
}

fn is_success_reason(reason: &str) -> bool {
    matches!(
        reason,
        "final_text" | "task_complete" | "plan_submitted" | "done"
    )
}

fn is_resumable_reason(reason: &str) -> bool {
    matches!(
        reason,
        "max_turns"
            | "repetition_guard"
            | "no_progress"
            | "repeated_failure"
            | "oscillation"
            | "provider_error"
            | "empty_completion"
            | "truncated_action"
            | "interrupted"
            | "hook_failed"
            | "needs_input"
    )
}

fn end_reason_is_legacy_pause(pause_reason: &Option<String>, saw_modern_turn: bool) -> bool {
    pause_reason.is_some() && !saw_modern_turn
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
        policy_selected_with_cap(protocol, ferric_core::DEFAULT_TRUNCATION_LIMIT)
    }

    fn policy_selected_with_cap(protocol: ActionProtocol, truncation_limit: usize) -> Event {
        Event::PolicySelected {
            tier: ferric_core::Tier::Nano,
            protocol,
            harness_policy: ferric_core::HarnessPolicy::Legacy,
            max_turns: 15,
            max_tools: 10,
            prompt_budget_tokens: 2_800,
            max_output_tokens: 512,
            truncation_limit,
            tier_source: ferric_core::TierSource::Params.label().to_string(),
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

    /// ADR-093: the projector takes its model-facing cap from the trace, so a
    /// replay of a run that used a non-default cap re-truncates at that cap
    /// rather than at the default. Before this, `replay()` built the projector
    /// with `TraceProjector::new()` and could only ever assume 4,000.
    #[test]
    fn replay_takes_the_truncation_cap_from_the_trace() {
        let long = "X".repeat(5_000);
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected_with_cap(ActionProtocol::ConstrainedJson, 500),
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
            // The trace keeps the FULL output (ADR-002); the cap applies only
            // when the context window is rebuilt from it.
            Event::ToolResult {
                id: "g-0-0".to_string(),
                name: "read_file".to_string(),
                output: long.clone(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
        ];
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        let result_msg = replayed.messages[3].text.as_deref().unwrap();
        let len = result_msg.chars().count();
        assert!(
            len < 1_000,
            "the traced cap of 500 must apply, but the rebuilt result carries \
             {len} chars — the default of {} was used instead",
            ferric_core::DEFAULT_TRUNCATION_LIMIT
        );
        // The control: without a cap in play the same shape is far longer, so
        // the assertion above is measuring the cap and not some other clipping.
        assert!(long.chars().count() > 4_000);
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
