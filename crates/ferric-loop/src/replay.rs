//! Reconstruct a still-incomplete session from its JSONL source of truth.
//!
//! Modern traces use `ActionsProposed` and `TurnCommitted` as an explicit
//! write-ahead protocol. A crash before dispatch is retryable, a tail whose
//! calls all have durable results is recoverable, and a dispatched call with
//! no result is reported as ambiguous instead of being silently repeated.
//! Pre-recovery traces retain their next-`TurnStart` commit semantics.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferric_core::{ActionProtocol, FerricError, HarnessPolicy, Message, UserInputRequest};
use ferric_trace::{
    ControllerCheckpointV1, Event, GuardTurn, ParsedEvent, RECOVERY_CHECKPOINT_VERSION,
    RecoveryCheckpointV1, TraceReadMode, TraceReader,
};
use thiserror::Error;

use crate::ControllerState;
use crate::projector::TraceProjector;
use crate::trace_structure::{TraceStructure, validate_recovery_checkpoint_shape};

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
    /// Concrete harness policy recorded by the source trace. Traces written
    /// before the field existed deserialize it as `Legacy`.
    pub harness_policy: HarnessPolicy,
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
    /// The intentional incomplete stop. Evidence traces use `process_crash`
    /// for an otherwise unclassified abrupt EOF; legacy abrupt EOFs retain
    /// `None` for compatibility.
    pub pause_reason: Option<String>,
    /// Durable controller truth for Evidence traces. Legacy traces deliberately
    /// carry `None` so absence can never default into trusted safety state.
    pub controller_checkpoint: Option<ControllerCheckpointV1>,
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
    #[error(
        "recorded harness policy {recorded} does not match requested harness policy {requested}"
    )]
    HarnessPolicyMismatch {
        recorded: HarnessPolicy,
        requested: HarnessPolicy,
    },
    #[error("recorded workspace {recorded} does not match requested workspace {requested}")]
    WorkspaceMismatch { recorded: String, requested: String },
    #[error("evidence trace has no projected controller checkpoint")]
    MissingControllerCheckpoint,
    #[error("legacy trace unexpectedly carries controller state")]
    UnexpectedControllerCheckpoint,
    #[error("invalid controller checkpoint: {0}")]
    InvalidControllerCheckpoint(String),
    #[error(
        "recorded required checks {recorded:?} do not match requested required checks {requested:?}"
    )]
    RequiredChecksMismatch {
        recorded: Vec<String>,
        requested: Vec<String>,
    },
}

/// Reconcile the one additive count introduced after recovery checkpoint v1
/// without weakening checkpoint anchoring. Only an actually omitted legacy
/// zero may inherit a nonzero count proven by typed `ControllerBlocked`
/// events already projected from the same trace. Explicit zeroes, nonzero
/// disagreements, raw-count changes, and every other state difference remain
/// hard errors.
fn reconcile_legacy_controller_block_counts(
    recorded: &RecoveryCheckpointV1,
    derived: &RecoveryCheckpointV1,
) -> Option<RecoveryCheckpointV1> {
    if recorded.guard_history.len() != derived.guard_history.len() {
        return None;
    }

    let mut reconciled = recorded.clone();
    for (stored, projected) in reconciled
        .guard_history
        .iter_mut()
        .zip(&derived.guard_history)
    {
        if stored.turn != projected.turn
            || stored.calls != projected.calls
            || stored.dispatched != projected.dispatched
            || stored.errored != projected.errored
        {
            return None;
        }
        if stored.controller_blocks != projected.controller_blocks {
            if stored.controller_blocks != 0
                || stored.controller_blocks_was_present
                || projected.controller_blocks == 0
            {
                return None;
            }
            stored.controller_blocks = projected.controller_blocks;
            stored.controller_blocks_was_present = true;
        }
    }

    (reconciled == *derived).then_some(reconciled)
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
    let mut harness_policy: Option<HarnessPolicy> = None;
    let mut saw_session_paused = false;
    let mut structure = TraceStructure::new();

    for record in TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)? {
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

        let mut event = match record.event {
            ParsedEvent::Known(event) => event,
            ParsedEvent::Unknown(_) => return Err(ReplayError::UnknownEvent { seq: record.seq }),
        };

        if let Event::RecoveryCheckpoint { state } = &event {
            validate_checkpoint(state)?;
        }

        structure
            .observe(&event)
            .map_err(ReplayError::InvalidStructure)?;

        if let Event::RecoveryCheckpoint { state } = &mut event
            && saw_state_base
        {
            let derived = projector.checkpoint();
            if *state != derived {
                *state =
                    reconcile_legacy_controller_block_counts(state, &derived).ok_or_else(|| {
                        ReplayError::InvalidStructure(
                            "recovery checkpoint differs from the projector-derived state anchor"
                                .to_string(),
                        )
                    })?;
                validate_checkpoint(state)?;
            }
        }

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
            Event::PolicySelected {
                harness_policy: recorded,
                ..
            } => {
                harness_policy = Some(*recorded);
            }
            Event::RecoveryCheckpoint { .. } => {
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
                saw_session_paused = true;
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
        let unresolved: Vec<(String, bool)> = pending
            .tool_calls
            .iter()
            .filter(|call| !result_ids.contains(call.id.as_str()))
            .map(|call| {
                (
                    format!("{}:{}", call.id, call.name),
                    is_control_call(&call.name),
                )
            })
            .collect();
        let crossed_external_dispatch = unresolved.iter().any(|(_, control)| !control);
        let unresolved_control_with_sibling_result =
            !pending.tool_results.is_empty() && unresolved.iter().any(|(_, control)| *control);
        if crossed_external_dispatch || unresolved_control_with_sibling_result {
            return Err(ReplayError::AmbiguousTail {
                turn: pending.turn,
                calls: unresolved.into_iter().map(|(call, _)| call).collect(),
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
    let harness_policy = harness_policy.ok_or(ReplayError::MissingSessionPrompt)?;
    let source_session = source_session.ok_or(ReplayError::MissingSessionPrompt)?;
    let controller_checkpoint = projected_controller_checkpoint(
        &structure,
        &projector,
        harness_policy,
        &mut pause_reason,
        saw_session_paused,
    )?;

    Ok(ReplayedState {
        messages: projector.messages,
        turns: projector.turns,
        next_turn: projector.next_turn,
        last_text: projector.last_text,
        protocol,
        harness_policy,
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
        controller_checkpoint,
    })
}

fn projected_controller_checkpoint(
    structure: &TraceStructure,
    projector: &TraceProjector,
    harness_policy: HarnessPolicy,
    pause_reason: &mut Option<String>,
    saw_session_paused: bool,
) -> Result<Option<ControllerCheckpointV1>, ReplayError> {
    let projected = structure.controller_checkpoint();
    match harness_policy {
        HarnessPolicy::Legacy => {
            if projected.is_some() {
                return Err(ReplayError::UnexpectedControllerCheckpoint);
            }
            Ok(None)
        }
        HarnessPolicy::Evidence => {
            let checkpoint = projected.ok_or(ReplayError::MissingControllerCheckpoint)?;
            let controller = ControllerState::from_checkpoint(&checkpoint)
                .map_err(|error| ReplayError::InvalidControllerCheckpoint(error.to_string()))?;
            let reason = pause_reason
                .clone()
                .unwrap_or_else(|| "process_crash".to_string());
            if pause_reason.is_none() {
                *pause_reason = Some(reason.clone());
            }
            let checkpoint = if saw_session_paused {
                if checkpoint.inherited_pause_reason.as_deref() != Some(reason.as_str()) {
                    return Err(ReplayError::InvalidControllerCheckpoint(
                        "durable pause reason differs from projected controller state".to_string(),
                    ));
                }
                checkpoint
            } else {
                controller
                    .checkpoint_for_pause(&reason)
                    .map_err(|error| ReplayError::InvalidControllerCheckpoint(error.to_string()))?
            };
            if checkpoint.mutation_epoch != projector.mutation_epoch
                || checkpoint.passed_checks != projector.passed_checks
            {
                return Err(ReplayError::InvalidStructure(
                    "projected core/controller mutation-check coordinates disagree".to_string(),
                ));
            }
            Ok(Some(checkpoint))
        }
        HarnessPolicy::EvidencePlanner => Err(ReplayError::InvalidStructure(
            "evidence_planner is unsupported until a planner trace protocol is defined".to_string(),
        )),
    }
}

pub(crate) fn resume_controller_state(
    state: &ReplayedState,
    required_checks: &[String],
) -> Result<Option<ControllerState>, ReplayError> {
    match state.harness_policy {
        HarnessPolicy::Legacy => {
            if state.controller_checkpoint.is_some() {
                return Err(ReplayError::UnexpectedControllerCheckpoint);
            }
            Ok(None)
        }
        HarnessPolicy::Evidence => {
            let checkpoint = state
                .controller_checkpoint
                .as_ref()
                .ok_or(ReplayError::MissingControllerCheckpoint)?;
            let inherited_reason =
                checkpoint
                    .inherited_pause_reason
                    .as_deref()
                    .ok_or_else(|| {
                        ReplayError::InvalidControllerCheckpoint(
                            "resumed evidence checkpoint omits its pause reason".to_string(),
                        )
                    })?;
            if state.pause_reason.as_deref() != Some(inherited_reason) {
                return Err(ReplayError::InvalidControllerCheckpoint(
                    "replayed pause reason differs from controller checkpoint".to_string(),
                ));
            }
            if checkpoint.required_checks != required_checks {
                return Err(ReplayError::RequiredChecksMismatch {
                    recorded: checkpoint.required_checks.clone(),
                    requested: required_checks.to_vec(),
                });
            }
            ControllerState::resume_conservatively(checkpoint)
                .map(Some)
                .map_err(|error| ReplayError::InvalidControllerCheckpoint(error.to_string()))
        }
        HarnessPolicy::EvidencePlanner => Err(ReplayError::InvalidStructure(
            "evidence_planner is unsupported until a planner trace protocol is defined".to_string(),
        )),
    }
}

/// Validate the operator-selected execution context that a trace must never
/// silently change across a resume boundary. An omitted harness policy is not
/// a mismatch: `run()` inherits the concrete policy recorded in the trace.
pub fn validate_resume_target(
    state: &ReplayedState,
    workspace: &Path,
    protocol: ActionProtocol,
    harness_policy: Option<HarnessPolicy>,
) -> Result<(), ReplayError> {
    if state.protocol != protocol {
        return Err(ReplayError::ProtocolMismatch {
            recorded: state.protocol,
            requested: protocol,
        });
    }

    if let Some(requested) = harness_policy
        && state.harness_policy != requested
    {
        return Err(ReplayError::HarnessPolicyMismatch {
            recorded: state.harness_policy,
            requested,
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
    if state.version != RECOVERY_CHECKPOINT_VERSION {
        return Err(ReplayError::UnsupportedCheckpoint(state.version));
    }
    validate_recovery_checkpoint_shape(state).map_err(ReplayError::InvalidStructure)
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
    use ferric_trace::{GuardTurn, JsonlSink, TurnBoundary};
    use serde_json::json;
    use std::io::Write as _;

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

    fn valid_checkpoint_with_history() -> ferric_trace::RecoveryCheckpointV1 {
        let calls = |turn: u32| {
            vec![ferric_core::ToolCall {
                id: format!("read-{turn}"),
                name: "read_file".to_string(),
                args: json!({"path": format!("{turn}.txt")}),
            }]
        };
        ferric_trace::RecoveryCheckpointV1 {
            version: RECOVERY_CHECKPOINT_VERSION,
            messages: vec![
                Message::system("system"),
                Message::user("task"),
                Message::assistant("turn zero"),
                Message::user("result zero"),
                Message::assistant("turn two"),
                Message::user("result two"),
            ],
            next_turn: 3,
            last_text: Some("turn two".to_string()),
            head_len: 2,
            committed_turn_starts: vec![
                TurnBoundary {
                    turn: 0,
                    message_index: 2,
                },
                TurnBoundary {
                    turn: 2,
                    message_index: 4,
                },
            ],
            guard_history: vec![
                GuardTurn {
                    turn: 0,
                    calls: calls(0),
                    dispatched: 1,
                    errored: 0,
                    controller_blocks: 0,
                    controller_blocks_was_present: true,
                },
                GuardTurn {
                    turn: 2,
                    calls: calls(2),
                    dispatched: 1,
                    errored: 0,
                    controller_blocks: 0,
                    controller_blocks_was_present: true,
                },
            ],
            nudged_for_no_action: false,
            truncated_once: false,
            last_input_tokens: Some(10),
            pending_input: None,
            mutation_epoch: 0,
            passed_checks: std::collections::BTreeMap::new(),
        }
    }

    fn evidence_policy_selected() -> Event {
        Event::PolicySelected {
            tier: ferric_core::Tier::Nano,
            protocol: ActionProtocol::NativeTools,
            harness_policy: HarnessPolicy::Evidence,
            max_turns: 15,
            max_tools: 10,
            prompt_budget_tokens: 2_800,
            max_output_tokens: 512,
            truncation_limit: ferric_core::DEFAULT_TRUNCATION_LIMIT,
            tier_source: ferric_core::TierSource::Params.label().to_string(),
        }
    }

    fn evidence_fresh_prefix(required_checks: &[&str]) -> (Vec<Event>, ControllerState) {
        let controller = ControllerState::new(
            HarnessPolicy::Evidence,
            required_checks.iter().map(|name| (*name).to_string()),
        )
        .unwrap();
        (
            vec![
                Event::SessionStart {
                    workspace: "/ws".to_string(),
                    resumed_from: None,
                },
                evidence_policy_selected(),
                session_prompt(),
                Event::ControllerCheckpoint {
                    state: controller.checkpoint(),
                },
            ],
            controller,
        )
    }

    fn projected_core(events: &[Event]) -> ferric_trace::RecoveryCheckpointV1 {
        let mut projector = TraceProjector::new();
        for event in events {
            projector.step(event);
        }
        projector.checkpoint()
    }

    fn append_pause(events: &mut Vec<Event>, controller: &ControllerState, reason: &str) {
        let core = projected_core(events);
        events.push(Event::SessionEnd {
            reason: reason.to_string(),
        });
        events.push(Event::RecoveryCheckpoint { state: core });
        events.push(Event::ControllerCheckpoint {
            state: controller.checkpoint_for_pause(reason).unwrap(),
        });
        events.push(Event::SessionPaused {
            reason: reason.to_string(),
        });
    }

    fn core_from_replayed(state: &ReplayedState) -> ferric_trace::RecoveryCheckpointV1 {
        ferric_trace::RecoveryCheckpointV1 {
            version: ferric_trace::RECOVERY_CHECKPOINT_VERSION,
            messages: state.messages.clone(),
            next_turn: state.next_turn,
            last_text: state.last_text.clone(),
            head_len: state.head_len,
            committed_turn_starts: state
                .committed_turn_starts
                .iter()
                .map(|&(turn, message_index)| ferric_trace::TurnBoundary {
                    turn,
                    message_index,
                })
                .collect(),
            guard_history: state.guard_history.clone(),
            nudged_for_no_action: state.nudged_for_no_action,
            truncated_once: state.truncated_once,
            last_input_tokens: state.last_input_tokens,
            pending_input: state.pending_input.clone(),
            mutation_epoch: state.mutation_epoch,
            passed_checks: state.passed_checks.clone(),
        }
    }

    fn evidence_resume_prefix(state: &ReplayedState) -> (Vec<Event>, ControllerState) {
        let controller = resume_controller_state(
            state,
            &state
                .controller_checkpoint
                .as_ref()
                .unwrap()
                .required_checks,
        )
        .unwrap()
        .unwrap();
        (
            vec![
                Event::SessionStart {
                    workspace: "/ws".to_string(),
                    resumed_from: Some(state.source_session.clone()),
                },
                evidence_policy_selected(),
                Event::RecoveryCheckpoint {
                    state: core_from_replayed(state),
                },
                Event::ControllerCheckpoint {
                    state: controller.checkpoint(),
                },
            ],
            controller,
        )
    }

    fn recovery_packet_event(controller: &ControllerState) -> Event {
        let reason = controller
            .checkpoint()
            .inherited_pause_reason
            .expect("resumed controller carries pause reason");
        let packet = controller.recovery_packet(&reason).unwrap();
        let message = ControllerState::render_recovery_packet(&packet).unwrap();
        Event::RecoveryPacketInjected { packet, message }
    }

    fn append_needs_input_turn(events: &mut Vec<Event>, turn: u32) {
        let call = ferric_core::ToolCall {
            id: format!("ask-{turn}"),
            name: "request_user_input".to_string(),
            args: json!({
                "question": "Which file?",
                "context": "Two candidates",
                "options": ["a.txt", "b.txt"]
            }),
        };
        events.extend([
            Event::TurnStart { turn },
            Event::TurnEnd {
                turn,
                text: Some("I need one decision.".to_string()),
                tool_call_count: 1,
                input_tokens: Some(20),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ActionsProposed {
                turn,
                calls: vec![call.clone()],
            },
            Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            },
            Event::ToolResult {
                id: call.id,
                name: call.name,
                output: "waiting for user input".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnCommitted {
                turn,
                dispatched: 1,
                errored: 0,
                stop_reason: Some("needs_input".to_string()),
                snapshot_commit: None,
            },
        ]);
    }

    fn append_controller_blocked_turn(
        events: &mut Vec<Event>,
        controller: &ControllerState,
        turn: u32,
    ) {
        let call = ferric_core::ToolCall {
            id: format!("blocked-{turn}"),
            name: "unsupported_mutation".to_string(),
            args: json!({"path": "target.txt"}),
        };
        let block = controller
            .unsupported_mutation_block(
                vec!["target.txt".to_string()],
                ferric_trace::UnsupportedMutationKindV1::UnsupportedOperation,
            )
            .unwrap();
        events.extend([
            Event::TurnStart { turn },
            Event::TurnEnd {
                turn,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(20),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ActionsProposed {
                turn,
                calls: vec![call.clone()],
            },
            Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            },
            Event::ControllerBlocked {
                turn,
                call_id: call.id.clone(),
                tool: call.name.clone(),
                block,
            },
            Event::ToolResult {
                id: call.id,
                name: call.name,
                output: "controller blocked".to_string(),
                is_error: true,
                duration_ms: 0,
            },
            Event::TurnCommitted {
                turn,
                dispatched: 1,
                errored: 1,
                stop_reason: None,
                snapshot_commit: None,
            },
        ]);
    }

    fn append_created_file_turn(
        events: &mut Vec<Event>,
        controller: &mut ControllerState,
        turn: u32,
    ) {
        let call = ferric_core::ToolCall {
            id: format!("write-{turn}"),
            name: "write_file".to_string(),
            args: json!({"path": "new.txt", "content": "new"}),
        };
        let effect = ferric_trace::WorkspaceEffectV1 {
            version: ferric_trace::CONTROLLER_RECORD_VERSION,
            mutation_epoch: controller.mutation_epoch() + 1,
            effects: vec![ferric_trace::PathEffectV1 {
                path: "new.txt".to_string(),
                kind: ferric_trace::PathEffectKind::Created,
                before_sha256: None,
                after_sha256: Some("b".repeat(64)),
                after_bytes: Some(3),
                after_lines: Some(1),
            }],
        };
        controller.apply_workspace_effect(turn, &effect).unwrap();
        events.extend([
            Event::TurnStart { turn },
            Event::TurnEnd {
                turn,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(20),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ActionsProposed {
                turn,
                calls: vec![call.clone()],
            },
            Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            },
            Event::WorkspaceEffectRecorded {
                turn,
                call_id: call.id.clone(),
                tool: call.name.clone(),
                effect: effect.clone(),
            },
            Event::WorkspaceMutation {
                turn,
                tool: call.name.clone(),
                mutation_epoch: effect.mutation_epoch,
            },
            Event::ToolResult {
                id: call.id,
                name: call.name,
                output: "wrote new.txt".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnCommitted {
                turn,
                dispatched: 1,
                errored: 0,
                stop_reason: None,
                snapshot_commit: None,
            },
        ]);
    }

    fn append_check_turn(
        events: &mut Vec<Event>,
        controller: &mut ControllerState,
        turn: u32,
        outcome: ferric_trace::VerificationOutcome,
        diagnostic_sha256: Option<String>,
    ) {
        let call = ferric_core::ToolCall {
            id: format!("check-{turn}"),
            name: "run_check".to_string(),
            args: json!({"name": "unit"}),
        };
        let check = ferric_trace::VerificationCheckV1 {
            version: ferric_trace::CONTROLLER_RECORD_VERSION,
            name: "unit".to_string(),
            mutation_epoch: controller.mutation_epoch(),
            attempt: controller.admit_check("unit").unwrap(),
            outcome,
            diagnostic_sha256,
        };
        controller.apply_verification_check(turn, &check).unwrap();
        events.extend([
            Event::TurnStart { turn },
            Event::TurnEnd {
                turn,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(20),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ActionsProposed {
                turn,
                calls: vec![call.clone()],
            },
            Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            },
            Event::VerificationCheckRecorded {
                turn,
                call_id: call.id.clone(),
                check: check.clone(),
            },
        ]);
        if outcome == ferric_trace::VerificationOutcome::Passed {
            events.push(Event::VerificationCheckPassed {
                turn,
                name: check.name.clone(),
                mutation_epoch: check.mutation_epoch,
            });
        }
        let is_error = outcome == ferric_trace::VerificationOutcome::Failed;
        events.extend([
            Event::ToolResult {
                id: call.id,
                name: call.name,
                output: if is_error {
                    "unit failed".to_string()
                } else {
                    "unit passed".to_string()
                },
                is_error,
                duration_ms: 1,
            },
            Event::TurnCommitted {
                turn,
                dispatched: 1,
                errored: u32::from(is_error),
                stop_reason: None,
                snapshot_commit: None,
            },
        ]);
    }

    #[test]
    fn replay_drops_only_a_torn_tail_then_preserves_dispatch_ambiguity() {
        let call = ferric_core::ToolCall {
            id: "write-0".to_string(),
            name: "write_file".to_string(),
            args: json!({"path": "new.txt", "content": "new"}),
        };
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
                input_tokens: Some(20),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ActionsProposed {
                turn: 0,
                calls: vec![call.clone()],
            },
            Event::ToolCall {
                id: call.id,
                name: call.name,
                args: call.args,
            },
        ];
        let (_dir, path) = write_trace(&events);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(br#"{"v":1,"ts_ms":2,"session":"s-1","seq":7,"event":{"type":"tool_result""#)
            .unwrap();

        assert!(matches!(
            replay(&path),
            Err(ReplayError::AmbiguousTail { turn: 0, calls })
                if calls == ["write-0:write_file"]
        ));
    }

    #[test]
    fn unresolved_terminal_control_with_resulted_sibling_is_ambiguous() {
        let complete = ferric_core::ToolCall {
            id: "complete-0".to_string(),
            name: crate::terminator::TASK_COMPLETE.to_string(),
            args: json!({"summary": "done"}),
        };
        let read = ferric_core::ToolCall {
            id: "read-0".to_string(),
            name: "read_file".to_string(),
            args: json!({"path": "a.txt"}),
        };
        let mut events = vec![
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
                input_tokens: Some(20),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ActionsProposed {
                turn: 0,
                calls: vec![complete.clone(), read.clone()],
            },
            Event::ToolCall {
                id: complete.id.clone(),
                name: complete.name.clone(),
                args: complete.args.clone(),
            },
            Event::ToolCall {
                id: read.id.clone(),
                name: read.name.clone(),
                args: read.args.clone(),
            },
            Event::ToolResult {
                id: read.id,
                name: read.name,
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 1,
            },
        ];
        let (_dir, path) = write_trace(&events);
        assert!(matches!(
            replay(&path),
            Err(ReplayError::AmbiguousTail { turn: 0, calls })
                if calls == ["complete-0:task_complete"]
        ));

        events.truncate(events.len() - 2);
        let (_dir, control_only_path) = write_trace(&events);
        let retryable = replay(&control_only_path).unwrap();
        assert_eq!(retryable.next_turn, 0);
        assert_eq!(retryable.messages, expected_prefix());
    }

    #[test]
    fn recovery_checkpoint_history_and_guard_coordinates_are_canonical() {
        let valid = valid_checkpoint_with_history();
        validate_checkpoint(&valid).unwrap();

        let mut invalid = Vec::new();
        let mut checkpoint = valid.clone();
        checkpoint.committed_turn_starts[0].message_index = 1;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.committed_turn_starts[1].message_index = checkpoint.messages.len();
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.committed_turn_starts[1].turn = 0;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.committed_turn_starts[1].message_index = 2;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.committed_turn_starts[1].turn = checkpoint.next_turn;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.guard_history[1].turn = 0;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.guard_history[1].turn = 1;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.guard_history[1].turn = checkpoint.next_turn;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.guard_history[0].calls.clear();
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.guard_history[0].dispatched = 2;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.guard_history[0].controller_blocks = 1;
        invalid.push(checkpoint);
        let mut checkpoint = valid.clone();
        checkpoint.guard_history[0].calls[0].name = crate::terminator::TASK_COMPLETE.to_string();
        checkpoint.guard_history[0].errored = 1;
        checkpoint.guard_history[0].controller_blocks = 1;
        invalid.push(checkpoint);
        let mut checkpoint = valid;
        checkpoint.guard_history[0].errored = 2;
        invalid.push(checkpoint);

        for checkpoint in invalid {
            assert!(matches!(
                validate_checkpoint(&checkpoint),
                Err(ReplayError::InvalidStructure(_))
            ));
        }
    }

    #[test]
    fn invalid_checkpoint_boundaries_fail_before_compaction_can_project_them() {
        let mut checkpoint = valid_checkpoint_with_history();
        checkpoint.committed_turn_starts[1].message_index = 1;
        let events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: Some("prior".to_string()),
            },
            policy_selected(ActionProtocol::NativeTools),
            Event::RecoveryCheckpoint { state: checkpoint },
            Event::HistoryCompacted {
                through_turn: 0,
                dropped_turns: 1,
                summary: "summary".to_string(),
            },
        ];
        let (_dir, path) = write_trace(&events);
        assert!(matches!(
            replay(&path),
            Err(ReplayError::InvalidStructure(_))
        ));

        let mut projector = TraceProjector::new();
        projector.protocol = Some(ActionProtocol::NativeTools);
        projector.messages = valid_checkpoint_with_history().messages;
        projector.head_len = 2;
        projector.committed_turn_starts = vec![(0, 2), (2, 1)];
        let before = projector.messages.clone();
        projector.step(&Event::HistoryCompacted {
            through_turn: 0,
            dropped_turns: 1,
            summary: "summary".to_string(),
        });
        assert_eq!(projector.messages, before);
    }

    #[test]
    fn later_pause_and_answer_checkpoints_must_match_projected_history_exactly() {
        let mut paused_events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            },
            policy_selected(ActionProtocol::NativeTools),
            session_prompt(),
        ];
        let mut forged_pause = projected_core(&paused_events);
        forged_pause
            .messages
            .push(Message::user("forged pause history"));
        paused_events.push(Event::SessionEnd {
            reason: "max_turns".to_string(),
        });
        paused_events.push(Event::RecoveryCheckpoint {
            state: forged_pause,
        });
        let (_dir, pause_path) = write_trace(&paused_events);
        assert!(matches!(
            replay(&pause_path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("projector-derived state anchor")
        ));

        let base = ferric_trace::RecoveryCheckpointV1 {
            version: RECOVERY_CHECKPOINT_VERSION,
            messages: expected_prefix(),
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
        let answer_events = vec![
            Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: Some("prior".to_string()),
            },
            policy_selected(ActionProtocol::NativeTools),
            Event::RecoveryCheckpoint {
                state: base.clone(),
            },
            Event::ResumePrompt {
                user: "answer".to_string(),
                media: Vec::new(),
            },
            Event::RecoveryCheckpoint { state: base },
        ];
        let (_dir, answer_path) = write_trace(&answer_events);
        assert!(matches!(
            replay(&answer_path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("projector-derived state anchor")
        ));
    }

    #[test]
    fn evidence_replay_requires_projected_state_and_derives_safe_crash_reason() {
        let (events, _) = evidence_fresh_prefix(&["unit"]);
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(replayed.pause_reason.as_deref(), Some("process_crash"));
        let checkpoint = replayed.controller_checkpoint.as_ref().unwrap();
        assert_eq!(checkpoint.required_checks, ["unit"]);
        assert_eq!(
            checkpoint.inherited_pause_reason.as_deref(),
            Some("process_crash")
        );
        assert_eq!(replayed.messages, expected_prefix());

        let mismatch = resume_controller_state(&replayed, &["lint".to_string()]).unwrap_err();
        assert!(matches!(
            mismatch,
            ReplayError::RequiredChecksMismatch { .. }
        ));
        let resumed = resume_controller_state(&replayed, &["unit".to_string()])
            .unwrap()
            .unwrap();
        assert!(resumed.file_evidence("missing.rs").is_none());

        let mut mismatched_reason = replayed;
        mismatched_reason.pause_reason = Some("max_turns".to_string());
        assert!(matches!(
            resume_controller_state(&mismatched_reason, &["unit".to_string()]),
            Err(ReplayError::InvalidControllerCheckpoint(message))
                if message.contains("pause reason differs")
        ));
    }

    #[test]
    fn evidence_replay_preserves_a_complete_intentional_pause_suffix() {
        let (mut events, controller) = evidence_fresh_prefix(&["unit"]);
        append_pause(&mut events, &controller, "max_turns");
        let expected = controller.checkpoint_for_pause("max_turns").unwrap();
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(replayed.pause_reason.as_deref(), Some("max_turns"));
        assert_eq!(replayed.controller_checkpoint, Some(expected));
    }

    #[test]
    fn old_checkpoint_omission_recovers_only_typed_controller_block_counts() {
        let (mut events, controller) = evidence_fresh_prefix(&[]);
        append_controller_blocked_turn(&mut events, &controller, 0);
        append_pause(&mut events, &controller, "max_turns");
        let (_dir, path) = write_trace(&events);
        let modern = std::fs::read_to_string(&path).unwrap();
        assert!(modern.contains("\"controller_blocks\":1"));

        let legacy = modern.replace(",\"controller_blocks\":1", "");
        std::fs::write(&path, legacy).unwrap();
        let replayed = replay(&path).unwrap();
        assert_eq!(replayed.guard_history[0].controller_blocks, 1);
        assert!(replayed.guard_history[0].controller_blocks_was_present);

        let explicit_zero = modern.replace("\"controller_blocks\":1", "\"controller_blocks\":0");
        std::fs::write(&path, explicit_zero).unwrap();
        assert!(matches!(
            replay(&path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("differs from typed event count")
        ));

        let explicit_null = modern.replace("\"controller_blocks\":1", "\"controller_blocks\":null");
        std::fs::write(&path, explicit_null).unwrap();
        let error = replay(&path).unwrap_err();
        assert!(matches!(error, ReplayError::Trace(_)), "{error:?}");

        let wrong_nonzero = modern.replace("\"controller_blocks\":1", "\"controller_blocks\":2");
        std::fs::write(&path, wrong_nonzero).unwrap();
        assert!(matches!(
            replay(&path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("incoherent dispatch/error counts")
        ));
    }

    #[test]
    fn fully_resulted_eof_tail_projects_controller_block_count() {
        let (mut events, controller) = evidence_fresh_prefix(&[]);
        append_controller_blocked_turn(&mut events, &controller, 0);
        assert!(matches!(
            events.pop(),
            Some(Event::TurnCommitted { turn: 0, .. })
        ));
        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();

        assert_eq!(replayed.next_turn, 1);
        assert_eq!(replayed.guard_history.len(), 1);
        assert_eq!(
            (
                replayed.guard_history[0].dispatched,
                replayed.guard_history[0].errored,
                replayed.guard_history[0].controller_blocks,
            ),
            (1, 1, 1)
        );
    }

    #[test]
    fn evidence_crash_prefix_retries_predispatch_without_inventing_an_effect() {
        let call = ferric_core::ToolCall {
            id: "write-0".to_string(),
            name: "write_file".to_string(),
            args: json!({"path": "new.txt", "content": "new"}),
        };
        let (mut safe_events, _) = evidence_fresh_prefix(&[]);
        safe_events.extend([
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
        ]);
        let (_dir, safe_path) = write_trace(&safe_events);
        let safe = replay(&safe_path).unwrap();
        assert_eq!(safe.next_turn, 0);
        assert_eq!(safe.mutation_epoch, 0);
        assert_eq!(safe.messages, expected_prefix());
        let checkpoint = safe.controller_checkpoint.unwrap();
        assert_eq!(checkpoint.mutation_epoch, 0);
        assert!(checkpoint.changed_paths.is_empty());
        assert!(checkpoint.file_evidence.is_empty());

        let mut ambiguous_events = safe_events;
        ambiguous_events.push(Event::ToolCall {
            id: call.id,
            name: call.name,
            args: call.args,
        });
        let (_dir, ambiguous_path) = write_trace(&ambiguous_events);
        assert!(matches!(
            replay(&ambiguous_path),
            Err(ReplayError::AmbiguousTail { turn: 0, calls })
                if calls == ["write-0:write_file"]
        ));

        // Once the result is durable, replay can finish the missing commit
        // without losing the already-projected controller effect.
        let (mut fully_resulted_events, mut controller) = evidence_fresh_prefix(&[]);
        append_created_file_turn(&mut fully_resulted_events, &mut controller, 0);
        assert!(matches!(
            fully_resulted_events.pop(),
            Some(Event::TurnCommitted { turn: 0, .. })
        ));
        let (_dir, fully_resulted_path) = write_trace(&fully_resulted_events);
        let fully_resulted = replay(&fully_resulted_path).unwrap();
        assert_eq!(fully_resulted.next_turn, 1);
        assert_eq!(fully_resulted.mutation_epoch, 1);
        let checkpoint = fully_resulted.controller_checkpoint.unwrap();
        assert_eq!(checkpoint.mutation_epoch, 1);
        assert_eq!(checkpoint.changed_paths, ["new.txt"]);
    }

    #[test]
    fn evidence_resume_packet_is_literal_history_after_base_or_generic_anchors() {
        let (mut paused_events, controller) = evidence_fresh_prefix(&["unit"]);
        append_pause(&mut paused_events, &controller, "max_turns");
        let (_dir, paused_path) = write_trace(&paused_events);
        let paused = replay(&paused_path).unwrap();

        let (mut direct_events, direct_controller) = evidence_resume_prefix(&paused);
        let direct_packet = recovery_packet_event(&direct_controller);
        let direct_message = match &direct_packet {
            Event::RecoveryPacketInjected { message, .. } => message.clone(),
            _ => unreachable!(),
        };
        direct_events.push(direct_packet);
        let (_dir, direct_path) = write_trace(&direct_events);
        let direct = replay(&direct_path).unwrap();
        assert_eq!(direct.messages.last(), Some(&Message::user(direct_message)));

        let (mut amended_events, amended_controller) = evidence_resume_prefix(&paused);
        amended_events.push(Event::ResumePrompt {
            user: "also update the docs".to_string(),
            media: Vec::new(),
        });
        amended_events.push(Event::RecoveryCheckpoint {
            state: projected_core(&amended_events),
        });
        amended_events.push(Event::ControllerCheckpoint {
            state: amended_controller.checkpoint(),
        });
        let amended_packet = recovery_packet_event(&amended_controller);
        let amended_message = match &amended_packet {
            Event::RecoveryPacketInjected { message, .. } => message.clone(),
            _ => unreachable!(),
        };
        amended_events.push(amended_packet);
        let (_dir, amended_path) = write_trace(&amended_events);
        let amended = replay(&amended_path).unwrap();
        assert_eq!(
            &amended.messages[amended.messages.len() - 2..],
            &[
                Message::user_with_media("also update the docs", Vec::new()),
                Message::user(amended_message)
            ]
        );
    }

    #[test]
    fn evidence_clarification_resume_anchors_the_answer_without_a_generic_packet() {
        let (mut paused_events, controller) = evidence_fresh_prefix(&[]);
        append_needs_input_turn(&mut paused_events, 0);
        append_pause(&mut paused_events, &controller, "needs_input");
        let (_dir, paused_path) = write_trace(&paused_events);
        let paused = replay(&paused_path).unwrap();
        assert_eq!(paused.pause_reason.as_deref(), Some("needs_input"));
        assert_eq!(
            paused
                .controller_checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.inherited_pause_reason.as_deref()),
            Some("needs_input")
        );
        assert_eq!(
            paused
                .pending_input
                .as_ref()
                .map(|request| request.question.as_str()),
            Some("Which file?")
        );

        let (mut resumed_events, resumed_controller) = evidence_resume_prefix(&paused);
        resumed_events.push(Event::ResumePrompt {
            user: "a.txt".to_string(),
            media: Vec::new(),
        });
        resumed_events.push(Event::RecoveryCheckpoint {
            state: projected_core(&resumed_events),
        });
        resumed_events.push(Event::ControllerCheckpoint {
            state: resumed_controller.checkpoint(),
        });
        assert!(
            resumed_events
                .iter()
                .all(|event| !matches!(event, Event::RecoveryPacketInjected { .. }))
        );

        let (_dir, resumed_path) = write_trace(&resumed_events);
        let resumed = replay(&resumed_path).unwrap();
        assert_eq!(resumed.pending_input, None);
        assert_eq!(
            resumed.messages.last(),
            Some(&Message::user_with_media("a.txt", Vec::new()))
        );
        assert_eq!(resumed.pause_reason.as_deref(), Some("process_crash"));
    }

    #[test]
    fn evidence_replay_accepts_safe_answer_eof_and_rejects_incomplete_anchor_windows() {
        let (mut paused_events, controller) = evidence_fresh_prefix(&[]);
        append_pause(&mut paused_events, &controller, "max_turns");
        let (_dir, paused_path) = write_trace(&paused_events);
        let paused = replay(&paused_path).unwrap();

        // ResumePrompt is itself a durable, replay-safe transition. An EOF
        // before its two explicit anchors must retain the answer for a later
        // resume-of-resume rather than ask the user twice.
        let (mut safe_events, _) = evidence_resume_prefix(&paused);
        safe_events.push(Event::ResumePrompt {
            user: "also update docs".to_string(),
            media: Vec::new(),
        });
        let (_dir, safe_path) = write_trace(&safe_events);
        let safe = replay(&safe_path).unwrap();
        assert_eq!(
            safe.messages.last(),
            Some(&Message::user_with_media("also update docs", Vec::new()))
        );
        assert_eq!(safe.pause_reason.as_deref(), Some("process_crash"));
        assert_eq!(
            safe.controller_checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.inherited_pause_reason.as_deref()),
            Some("process_crash")
        );

        let (fresh_without_controller, _) = evidence_fresh_prefix(&[]);
        let mut fresh_without_controller = fresh_without_controller;
        fresh_without_controller.pop();
        let (_dir, path) = write_trace(&fresh_without_controller);
        assert!(matches!(
            replay(&path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("required ControllerCheckpoint")
        ));

        let (resume_without_controller, _) = evidence_resume_prefix(&paused);
        let mut resume_without_controller = resume_without_controller;
        resume_without_controller.pop();
        let (_dir, path) = write_trace(&resume_without_controller);
        assert!(matches!(
            replay(&path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("required ControllerCheckpoint")
        ));

        let (mut answer_without_controller, _) = evidence_resume_prefix(&paused);
        answer_without_controller.push(Event::ResumePrompt {
            user: "also update docs".to_string(),
            media: Vec::new(),
        });
        answer_without_controller.push(Event::RecoveryCheckpoint {
            state: projected_core(&answer_without_controller),
        });
        let (_dir, path) = write_trace(&answer_without_controller);
        assert!(matches!(
            replay(&path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("required ControllerCheckpoint")
        ));

        let (mut unsupported, _) = evidence_fresh_prefix(&[]);
        let Some(Event::ControllerCheckpoint { state }) = unsupported.last_mut() else {
            unreachable!()
        };
        state.version += 1;
        let (_dir, path) = write_trace(&unsupported);
        assert!(matches!(
            replay(&path),
            Err(ReplayError::InvalidStructure(message))
                if message.contains("unsupported controller checkpoint version")
        ));
    }

    #[test]
    fn evidence_resume_of_resume_uses_the_latest_projected_controller_state() {
        let (mut paused_events, controller) = evidence_fresh_prefix(&["unit"]);
        append_pause(&mut paused_events, &controller, "max_turns");
        let (_dir, paused_path) = write_trace(&paused_events);
        let paused = replay(&paused_path).unwrap();

        let (mut first_resume_events, mut first_resume_controller) =
            evidence_resume_prefix(&paused);
        first_resume_events.push(recovery_packet_event(&first_resume_controller));
        let mutation_turn = paused.next_turn;
        append_created_file_turn(
            &mut first_resume_events,
            &mut first_resume_controller,
            mutation_turn,
        );
        append_check_turn(
            &mut first_resume_events,
            &mut first_resume_controller,
            mutation_turn + 1,
            ferric_trace::VerificationOutcome::Failed,
            Some("f".repeat(64)),
        );
        let (_dir, first_resume_path) = write_trace(&first_resume_events);
        let first_resume = replay(&first_resume_path).unwrap();
        let first_checkpoint = first_resume.controller_checkpoint.as_ref().unwrap();
        assert_eq!(first_checkpoint.mutation_epoch, 1);
        assert_eq!(first_checkpoint.changed_paths, ["new.txt"]);
        assert_eq!(first_checkpoint.repair_paths, ["new.txt"]);
        assert_eq!(
            first_checkpoint
                .last_failed_check
                .as_ref()
                .map(|failure| (failure.name.as_str(), failure.mutation_epoch)),
            Some(("unit", 1))
        );
        assert_eq!(
            first_checkpoint.inherited_pause_reason.as_deref(),
            Some("process_crash")
        );
        assert_eq!(first_resume.mutation_epoch, 1);

        let (mut second_resume_events, second_resume_controller) =
            evidence_resume_prefix(&first_resume);
        second_resume_events.push(recovery_packet_event(&second_resume_controller));
        let (_dir, second_resume_path) = write_trace(&second_resume_events);
        let second_resume = replay(&second_resume_path).unwrap();
        let second_checkpoint = second_resume.controller_checkpoint.as_ref().unwrap();
        assert_eq!(second_checkpoint.mutation_epoch, 1);
        assert_eq!(second_checkpoint.changed_paths, ["new.txt"]);
        assert_eq!(second_checkpoint.repair_paths, ["new.txt"]);
        assert_eq!(
            second_checkpoint
                .last_failed_check
                .as_ref()
                .map(|failure| (failure.name.as_str(), failure.mutation_epoch)),
            Some(("unit", 1))
        );
        assert_eq!(
            second_checkpoint.inherited_pause_reason.as_deref(),
            Some("process_crash")
        );
        assert_eq!(second_checkpoint.required_checks, ["unit"]);
        assert_eq!(second_checkpoint.harness_policy, HarnessPolicy::Evidence);
        let inherited_file = second_checkpoint
            .file_evidence
            .iter()
            .find(|evidence| evidence.path == "new.txt")
            .unwrap();
        assert!(!inherited_file.fresh);
        assert!(!inherited_file.complete);
        assert!(inherited_file.covered_ranges.is_empty());
        assert_eq!(second_resume.mutation_epoch, 1);
        assert_eq!(second_resume.harness_policy, HarnessPolicy::Evidence);
        assert_eq!(second_resume.next_turn, mutation_turn + 2);
        assert_eq!(
            second_resume
                .messages
                .iter()
                .filter(|message| message
                    .text
                    .as_deref()
                    .is_some_and(|text| text.starts_with("[Ferric recovery packet v")))
                .count(),
            2
        );
    }

    #[test]
    fn evidence_core_and_controller_projection_keep_effect_and_check_parity() {
        let (mut events, mut controller) = evidence_fresh_prefix(&["unit"]);
        append_check_turn(
            &mut events,
            &mut controller,
            0,
            ferric_trace::VerificationOutcome::Passed,
            None,
        );
        append_created_file_turn(&mut events, &mut controller, 1);
        append_check_turn(
            &mut events,
            &mut controller,
            2,
            ferric_trace::VerificationOutcome::Failed,
            Some("f".repeat(64)),
        );
        append_pause(&mut events, &controller, "max_turns");
        let expected = controller.checkpoint_for_pause("max_turns").unwrap();

        let (_dir, path) = write_trace(&events);
        let replayed = replay(&path).unwrap();
        assert_eq!(replayed.mutation_epoch, 1);
        assert!(replayed.passed_checks.is_empty());
        assert_eq!(replayed.controller_checkpoint, Some(expected));
        let checkpoint = replayed.controller_checkpoint.as_ref().unwrap();
        assert!(checkpoint.passed_checks.is_empty());
        assert_eq!(checkpoint.check_executions.len(), 2);
        assert_eq!(checkpoint.check_executions[1].attempt, 2);
        assert_eq!(checkpoint.changed_paths, ["new.txt"]);
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
        assert_eq!(replayed.pause_reason, None);
        assert_eq!(replayed.controller_checkpoint, None);
    }

    #[test]
    fn pre_evidence_trace_fixture_replays_as_legacy() {
        // Literal wire data copied in the shape emitted before Sprint 113:
        // PolicySelected has no `harness_policy`, and the trace contains no
        // controller events or controller checkpoint from which to invent
        // evidence state.
        const PRE_EVIDENCE_TRACE: &str = concat!(
            r#"{"v":1,"ts_ms":1,"session":"pre-evidence","seq":0,"event":{"type":"session_start","workspace":"/ws"}}"#,
            "\n",
            r#"{"v":1,"ts_ms":2,"session":"pre-evidence","seq":1,"event":{"type":"policy_selected","tier":"nano","protocol":"constrained_json","max_turns":15,"max_tools":10,"prompt_budget_tokens":2800,"max_output_tokens":512,"truncation_limit":4000,"tier_source":"params"}}"#,
            "\n",
            r#"{"v":1,"ts_ms":3,"session":"pre-evidence","seq":2,"event":{"type":"session_prompt","system":"You are Ferric.","user":"do the task"}}"#,
            "\n",
            r#"{"v":1,"ts_ms":4,"session":"pre-evidence","seq":3,"event":{"type":"turn_start","turn":0}}"#,
            "\n",
            r#"{"v":1,"ts_ms":5,"session":"pre-evidence","seq":4,"event":{"type":"turn_end","turn":0,"text":"{\"tool\":\"read_file\",\"args\":{\"path\":\"a.txt\"}}","tool_call_count":0,"input_tokens":50,"output_tokens":10,"truncated":false}}"#,
            "\n",
            r#"{"v":1,"ts_ms":6,"session":"pre-evidence","seq":5,"event":{"type":"tool_call","id":"g-0-0","name":"read_file","args":{"path":"a.txt"}}}"#,
            "\n",
            r#"{"v":1,"ts_ms":7,"session":"pre-evidence","seq":6,"event":{"type":"tool_result","id":"g-0-0","name":"read_file","output":"contents","is_error":false,"duration_ms":1}}"#,
            "\n",
            r#"{"v":1,"ts_ms":8,"session":"pre-evidence","seq":7,"event":{"type":"turn_start","turn":1}}"#,
            "\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pre-evidence.jsonl");
        std::fs::write(&path, PRE_EVIDENCE_TRACE).unwrap();

        let replayed = replay(&path).unwrap();
        assert_eq!(replayed.harness_policy, HarnessPolicy::Legacy);
        assert_eq!(replayed.controller_checkpoint, None);
        assert_eq!(replayed.mutation_epoch, 0);
        assert!(replayed.passed_checks.is_empty());
        assert_eq!(replayed.turns, 1);
        assert_eq!(replayed.next_turn, 1);
        assert_eq!(replayed.protocol, ActionProtocol::ConstrainedJson);
        assert_eq!(replayed.messages.len(), 4);
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
