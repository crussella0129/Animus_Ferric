//! Side-effect-free validation for Ferric JSONL traces.
//!
//! A trace is untrusted input. Verification therefore validates the recorded
//! transcript itself; it never rebuilds a provider, registry, or workspace and
//! never dispatches trace-authored tool calls.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use ferric_core::ActionProtocol;
use ferric_trace::{Event, ParsedEvent, TRACE_SCHEMA_VERSION, TraceReader};

#[derive(Debug)]
struct TurnState {
    number: u32,
    ended: bool,
    modern: bool,
    declared_calls: u32,
    calls: BTreeMap<String, RecordedCall>,
    pre_dispatch_stop: Option<PreDispatchStop>,
}

#[derive(Debug)]
struct RecordedCall {
    name: String,
    has_result: bool,
    is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreDispatchStop {
    Repetition,
    NoProgress,
    Oscillation,
}

impl PreDispatchStop {
    fn reason(self) -> &'static str {
        match self {
            Self::Repetition => "repetition_guard",
            Self::NoProgress => "no_progress",
            Self::Oscillation => "oscillation",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct VerificationSummary {
    records: usize,
    turns: usize,
    tool_calls: usize,
    unknown_events: usize,
    stop_reason: Option<String>,
}

#[derive(Debug, Default)]
struct VerificationState {
    structure: ferric_loop::TraceStructure,
    session: Option<String>,
    next_seq: u64,
    saw_start: bool,
    saw_policy: bool,
    protocol: Option<ActionProtocol>,
    current_turn: Option<TurnState>,
    last_turn: Option<u32>,
    saw_task_complete: bool,
    saw_submit_plan: bool,
    expected_stop: Option<&'static str>,
    saw_modern_turn: bool,
    paused_reason: Option<String>,
    checkpoint_after_stop: bool,
    mutation_epoch: u64,
    summary: VerificationSummary,
}

pub fn trace_verify(path: &Path) -> ExitCode {
    match verify_trace(path) {
        Ok(summary) => {
            let stop = summary.stop_reason.as_deref().unwrap_or("interrupted");
            let unknown = if summary.unknown_events == 0 {
                String::new()
            } else {
                format!(", {} unknown event(s) skipped", summary.unknown_events)
            };
            println!(
                "Trace verification successful: {} record(s), {} turn(s), {} tool call(s), stop={stop}{unknown}. No tools were executed.",
                summary.records, summary.turns, summary.tool_calls
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Trace verification failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn verify_trace(path: &Path) -> Result<VerificationSummary, String> {
    let reader = TraceReader::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut state = VerificationState::default();

    for (index, record) in reader.enumerate() {
        let record = record.map_err(|error| format!("record {index}: {error}"))?;
        state.summary.records += 1;

        if record.v != TRACE_SCHEMA_VERSION {
            return Err(format!(
                "record {index} uses schema v{}, expected v{}",
                record.v, TRACE_SCHEMA_VERSION
            ));
        }
        if record.seq != state.next_seq {
            return Err(format!(
                "record {index} has sequence {}, expected {}",
                record.seq, state.next_seq
            ));
        }
        state.next_seq += 1;

        match &state.session {
            Some(session) if session != &record.session => {
                return Err(format!(
                    "record {index} changes session from {session:?} to {:?}",
                    record.session
                ));
            }
            None => state.session = Some(record.session.clone()),
            _ => {}
        }

        match record.event {
            ParsedEvent::Known(event) => validate_event(index, event, &mut state)?,
            ParsedEvent::Unknown(_) => state.summary.unknown_events += 1,
        }
    }

    if state.summary.records == 0 {
        return Err("trace contains no records".to_string());
    }
    if !state.saw_start {
        return Err("trace has no session_start event".to_string());
    }
    if !state.saw_policy {
        return Err("trace has no policy_selected event".to_string());
    }
    state
        .structure
        .finish()
        .map_err(|error| format!("recovery structure: {error}"))?;
    finish_turn(&mut state, true)?;
    validate_stop_terminator(&state)?;

    Ok(state.summary)
}

fn validate_event(index: usize, event: Event, state: &mut VerificationState) -> Result<(), String> {
    state
        .structure
        .observe(&event)
        .map_err(|error| format!("record {index} recovery structure: {error}"))?;

    if state.summary.stop_reason.is_some()
        && !matches!(
            event,
            Event::Note { .. } | Event::RecoveryCheckpoint { .. } | Event::SessionPaused { .. }
        )
    {
        return Err(format!(
            "record {index} contains an event after session_end: {event:?}"
        ));
    }

    match event {
        Event::SessionStart { .. } => {
            if index != 0 {
                return Err(format!(
                    "session_start must be record 0, found at record {index}"
                ));
            }
            if state.saw_start {
                return Err("trace contains more than one session_start".to_string());
            }
            state.saw_start = true;
        }
        Event::PolicySelected { protocol, .. } => {
            require_started(index, state)?;
            if state.saw_policy {
                return Err("trace contains more than one policy_selected".to_string());
            }
            if state.current_turn.is_some() {
                return Err("policy_selected appears after turns began".to_string());
            }
            state.saw_policy = true;
            state.protocol = Some(protocol);
        }
        Event::SessionPrompt { .. } | Event::ResumePrompt { .. } | Event::PromptComposed { .. } => {
            require_started(index, state)?;
            if state.current_turn.is_some() {
                return Err(format!(
                    "record {index} contains prompt metadata inside a turn"
                ));
            }
        }
        Event::TurnStart { turn } => {
            require_policy(index, state)?;
            finish_turn(state, false)?;
            if let Some(previous) = state.last_turn
                && turn <= previous
            {
                return Err(format!(
                    "record {index} starts turn {turn} after turn {previous}"
                ));
            }
            state.last_turn = Some(turn);
            state.current_turn = Some(TurnState {
                number: turn,
                ended: false,
                modern: false,
                declared_calls: 0,
                calls: BTreeMap::new(),
                pre_dispatch_stop: None,
            });
        }
        Event::PromptAssembled { turn, .. } => {
            let current = current_turn_mut(index, state)?;
            if current.number != turn {
                return Err(format!(
                    "record {index} names turn {turn}, but active turn is {}",
                    current.number
                ));
            }
            if current.ended {
                return Err(format!(
                    "record {index} assembles a prompt after turn {turn} ended"
                ));
            }
        }
        Event::TurnEnd {
            turn,
            tool_call_count,
            ..
        } => {
            let current = current_turn_mut(index, state)?;
            if current.number != turn {
                return Err(format!(
                    "record {index} names turn {turn}, but active turn is {}",
                    current.number
                ));
            }
            if current.ended {
                return Err(format!("turn {turn} has more than one turn_end"));
            }
            current.ended = true;
            current.declared_calls = tool_call_count;
        }
        Event::ActionsProposed { turn, calls } => {
            let is_native = state.protocol == Some(ActionProtocol::NativeTools);
            let current = current_turn_mut(index, state)?;
            if current.number != turn || !current.ended {
                return Err(format!(
                    "record {index} proposes actions outside ended turn {turn}"
                ));
            }
            if is_native && calls.len() != current.declared_calls as usize {
                return Err(format!(
                    "turn {turn} proposed {} actions but declared {}",
                    calls.len(),
                    current.declared_calls
                ));
            }
            current.modern = true;
            state.saw_modern_turn = true;
        }
        Event::ToolCall { id, name, .. } => {
            let current = current_turn_mut(index, state)?;
            if !current.ended {
                return Err(format!(
                    "record {index} dispatches tool {name:?} before turn_end"
                ));
            }
            if current
                .calls
                .insert(
                    id.clone(),
                    RecordedCall {
                        name,
                        has_result: false,
                        is_error: false,
                    },
                )
                .is_some()
            {
                return Err(format!("record {index} repeats tool-call id {id:?}"));
            }
            state.summary.tool_calls += 1;
        }
        Event::ToolResult {
            id, name, is_error, ..
        } => {
            let current = current_turn_mut(index, state)?;
            let call = current.calls.get_mut(&id).ok_or_else(|| {
                format!("record {index} has a result for unknown tool-call id {id:?}")
            })?;
            if call.name != name {
                return Err(format!(
                    "record {index} result name {name:?} does not match call {:?}",
                    call.name
                ));
            }
            if call.has_result {
                return Err(format!(
                    "record {index} repeats the result for tool-call id {id:?}"
                ));
            }
            call.has_result = true;
            call.is_error = is_error;
        }
        Event::WorkspaceMutation {
            turn,
            mutation_epoch,
            ..
        } => {
            let current = current_turn_mut(index, state)?;
            if current.number != turn || !current.ended {
                return Err(format!(
                    "record {index} records a mutation outside ended turn {turn}"
                ));
            }
            if mutation_epoch != state.mutation_epoch.saturating_add(1) {
                return Err(format!(
                    "record {index} advances mutation epoch from {} to {mutation_epoch}",
                    state.mutation_epoch
                ));
            }
            state.mutation_epoch = mutation_epoch;
        }
        Event::VerificationCheckPassed {
            turn,
            mutation_epoch,
            ..
        } => {
            let current = current_turn_mut(index, state)?;
            if current.number != turn || !current.ended {
                return Err(format!(
                    "record {index} records check evidence outside ended turn {turn}"
                ));
            }
            if mutation_epoch != state.mutation_epoch {
                return Err(format!(
                    "record {index} records check evidence at epoch {mutation_epoch}, current epoch is {}",
                    state.mutation_epoch
                ));
            }
        }
        Event::CompletionGate {
            mutation_epoch,
            required_checks,
            fresh_checks,
            decision,
        } => {
            let current = current_turn_mut(index, state)?;
            if !current.ended || mutation_epoch != state.mutation_epoch {
                return Err(format!(
                    "record {index} has an out-of-state completion gate"
                ));
            }
            if !matches!(decision.as_str(), "passed" | "blocked")
                || fresh_checks
                    .iter()
                    .any(|check| !required_checks.contains(check))
                || (decision == "passed" && fresh_checks.len() != required_checks.len())
                || (decision == "blocked" && fresh_checks.len() == required_checks.len())
            {
                return Err(format!(
                    "record {index} has inconsistent completion evidence"
                ));
            }
        }
        Event::TurnCommitted {
            turn,
            dispatched,
            errored,
            ..
        } => {
            let current = current_turn_mut(index, state)?;
            if current.number != turn || !current.ended {
                return Err(format!("record {index} commits outside ended turn {turn}"));
            }
            let result_count = current
                .calls
                .values()
                .filter(|call| call.has_result)
                .count() as u32;
            let error_count = current
                .calls
                .values()
                .filter(|call| call.has_result && call.is_error)
                .count() as u32;
            if dispatched != result_count || errored != error_count {
                return Err(format!(
                    "turn {turn} commit reports {dispatched}/{errored} dispatched/errors, recorded {result_count}/{error_count}"
                ));
            }
            state.saw_modern_turn = true;
            finish_turn(state, false)?;
        }
        Event::SessionEnd { reason } => {
            require_policy(index, state)?;
            state.summary.stop_reason = Some(reason);
            finish_turn(state, false)?;
        }
        Event::RecoveryCheckpoint { state: checkpoint } => {
            require_policy(index, state)?;
            if checkpoint.version != ferric_trace::RECOVERY_CHECKPOINT_VERSION {
                return Err(format!(
                    "record {index} has unsupported checkpoint version {}",
                    checkpoint.version
                ));
            }
            if checkpoint.head_len > checkpoint.messages.len() {
                return Err(format!("record {index} checkpoint head exceeds messages"));
            }
            if checkpoint
                .passed_checks
                .values()
                .any(|epoch| *epoch > checkpoint.mutation_epoch)
            {
                return Err(format!(
                    "record {index} checkpoint contains future check evidence"
                ));
            }
            if state.last_turn.is_some() && checkpoint.mutation_epoch != state.mutation_epoch {
                return Err(format!(
                    "record {index} checkpoint mutation epoch {} differs from projected {}",
                    checkpoint.mutation_epoch, state.mutation_epoch
                ));
            }
            state.mutation_epoch = checkpoint.mutation_epoch;
            if state.summary.stop_reason.is_some() {
                state.checkpoint_after_stop = true;
            }
        }
        Event::SessionPaused { reason } => {
            let ended = state.summary.stop_reason.as_deref().ok_or_else(|| {
                format!("record {index} pauses before session_end established a reason")
            })?;
            if ended != reason {
                return Err(format!(
                    "record {index} pause reason {reason:?} differs from session_end {ended:?}"
                ));
            }
            if !state.checkpoint_after_stop {
                return Err(format!(
                    "record {index} pauses without a recovery checkpoint"
                ));
            }
            state.paused_reason = Some(reason);
        }
        Event::RepetitionGuard { action } => {
            record_pre_dispatch_stop(index, state, &action, PreDispatchStop::Repetition)?
        }
        Event::NoProgressGuard { action } => {
            record_pre_dispatch_stop(index, state, &action, PreDispatchStop::NoProgress)?
        }
        Event::OscillationGuard { action } => {
            record_pre_dispatch_stop(index, state, &action, PreDispatchStop::Oscillation)?
        }
        Event::ConstraintApplied { .. }
        | Event::FailureGuard { .. }
        | Event::PermissionCheck { .. }
        | Event::ObservationRecorded { .. }
        | Event::ControllerBlocked { .. }
        | Event::WorkspaceEffectRecorded { .. }
        | Event::VerificationCheckRecorded { .. }
        | Event::ControllerCheckpoint { .. }
        | Event::RecoveryPacketInjected { .. }
        | Event::HistoryCompacted { .. }
        | Event::Note { .. } => {
            require_started(index, state)?;
        }
    }
    Ok(())
}

fn require_started(index: usize, state: &VerificationState) -> Result<(), String> {
    if state.saw_start {
        Ok(())
    } else {
        Err(format!("record {index} appears before session_start"))
    }
}

fn require_policy(index: usize, state: &VerificationState) -> Result<(), String> {
    require_started(index, state)?;
    if state.saw_policy {
        Ok(())
    } else {
        Err(format!("record {index} appears before policy_selected"))
    }
}

fn current_turn_mut(index: usize, state: &mut VerificationState) -> Result<&mut TurnState, String> {
    state
        .current_turn
        .as_mut()
        .ok_or_else(|| format!("record {index} is turn-scoped but no turn is active"))
}

fn record_pre_dispatch_stop(
    index: usize,
    state: &mut VerificationState,
    action: &str,
    stop: PreDispatchStop,
) -> Result<(), String> {
    let current = current_turn_mut(index, state)?;
    if !current.ended {
        return Err(format!(
            "record {index} contains a guard decision before turn {} ended",
            current.number
        ));
    }
    match action {
        "warned" => Ok(()),
        "stopped" => {
            if let Some(previous) = current.pre_dispatch_stop {
                return Err(format!(
                    "record {index} contains guard stop {} after {} already stopped the turn",
                    stop.reason(),
                    previous.reason()
                ));
            }
            current.pre_dispatch_stop = Some(stop);
            Ok(())
        }
        other => Err(format!(
            "record {index} contains unknown {} guard action {other:?}",
            stop.reason()
        )),
    }
}

fn finish_turn(state: &mut VerificationState, eof: bool) -> Result<(), String> {
    let Some(turn) = state.current_turn.take() else {
        return Ok(());
    };
    if !turn.ended {
        if state.summary.stop_reason.as_deref() == Some("provider_error") && turn.calls.is_empty() {
            state.summary.turns += 1;
            return Ok(());
        }
        return Err(format!("turn {} has no turn_end", turn.number));
    }

    if let Some(stop) = turn.pre_dispatch_stop
        && state.expected_stop.replace(stop.reason()).is_some()
    {
        return Err("trace contains more than one pre-dispatch guard stop".to_string());
    }

    if state.protocol == Some(ActionProtocol::NativeTools) {
        let declared = turn.declared_calls as usize;
        let retryable_before_dispatch = eof && turn.modern && turn.calls.is_empty();
        let guarded_before_dispatch =
            turn.pre_dispatch_stop.is_some() && turn.calls.is_empty() && declared > 0;
        if turn.calls.len() != declared && !guarded_before_dispatch && !retryable_before_dispatch {
            return Err(format!(
                "turn {} declares {} tool call(s), but {} were recorded",
                turn.number,
                turn.declared_calls,
                turn.calls.len()
            ));
        }
        if turn.pre_dispatch_stop.is_some() && !guarded_before_dispatch {
            return Err(format!(
                "turn {} records a pre-dispatch guard stop with an impossible native call count",
                turn.number
            ));
        }
    }

    for (id, call) in turn.calls {
        if call.name == ferric_loop::TASK_COMPLETE {
            state.saw_task_complete = true;
        } else if call.name == ferric_loop::SUBMIT_PLAN {
            state.saw_submit_plan = true;
        } else if !call.has_result {
            return Err(format!(
                "turn {} tool call {id:?} ({:?}) has no tool_result",
                turn.number, call.name
            ));
        }
    }
    state.summary.turns += 1;
    Ok(())
}

fn validate_stop_terminator(state: &VerificationState) -> Result<(), String> {
    if let Some(expected) = state.expected_stop
        && state.summary.stop_reason.as_deref() != Some(expected)
    {
        return Err(format!(
            "pre-dispatch guard stop requires session_end({expected}), found {:?}",
            state.summary.stop_reason.as_deref()
        ));
    }
    match state.summary.stop_reason.as_deref() {
        Some("task_complete") if !state.saw_task_complete => {
            Err("session ended task_complete without a task_complete tool call".to_string())
        }
        Some("plan_submitted") if !state.saw_submit_plan => {
            Err("session ended plan_submitted without a submit_plan tool call".to_string())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::{Tier, TierSource};
    use ferric_trace::JsonlSink;
    use serde_json::json;

    fn write_trace(path: &Path, events: impl IntoIterator<Item = Event>) {
        let mut sink = JsonlSink::open(path, "verify-test").unwrap();
        for event in events {
            sink.write_event(event).unwrap();
        }
    }

    fn policy() -> Event {
        Event::PolicySelected {
            tier: Tier::Nano,
            protocol: ActionProtocol::NativeTools,
            harness_policy: ferric_core::HarnessPolicy::Legacy,
            max_turns: 3,
            max_tools: 8,
            prompt_budget_tokens: 2_800,
            max_output_tokens: 512,
            truncation_limit: ferric_core::DEFAULT_TRUNCATION_LIMIT,
            tier_source: TierSource::Params.label().to_string(),
        }
    }

    fn prefix(workspace: &Path) -> Vec<Event> {
        vec![
            Event::SessionStart {
                workspace: workspace.display().to_string(),
                resumed_from: None,
            },
            policy(),
            Event::SessionPrompt {
                system: "system".to_string(),
                user: "user".to_string(),
                media: Vec::new(),
            },
        ]
    }

    fn modern_call(id: &str, name: &str) -> ferric_core::ToolCall {
        ferric_core::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            args: json!({"path": "a.txt"}),
        }
    }

    fn checkpoint() -> ferric_trace::RecoveryCheckpointV1 {
        ferric_trace::RecoveryCheckpointV1 {
            version: ferric_trace::RECOVERY_CHECKPOINT_VERSION,
            messages: vec![
                ferric_core::Message::system("system"),
                ferric_core::Message::user("task"),
            ],
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
            passed_checks: BTreeMap::new(),
        }
    }

    #[test]
    fn rejects_a_next_turn_after_an_uncommitted_modern_turn() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("uncommitted-modern.jsonl");
        let call = modern_call("read-1", "read_file");
        let mut events = prefix(dir.path());
        events.extend([
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
                output: "read".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
        ]);
        write_trace(&trace, events);

        let error = verify_trace(&trace).unwrap_err();
        assert!(error.contains("without TurnCommitted"), "{error}");
    }

    #[test]
    fn rejects_dispatch_that_differs_from_the_proposed_batch() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("proposal-mismatch.jsonl");
        let mut events = prefix(dir.path());
        events.extend([
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
                calls: vec![modern_call("read-1", "read_file")],
            },
            Event::ToolCall {
                id: "write-1".to_string(),
                name: "write_file".to_string(),
                args: json!({"path": "a.txt"}),
            },
        ]);
        write_trace(&trace, events);

        let error = verify_trace(&trace).unwrap_err();
        assert!(error.contains("dispatched call"), "{error}");
    }

    #[test]
    fn rejects_a_checkpoint_inside_an_active_turn() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("checkpoint-in-turn.jsonl");
        let mut events = prefix(dir.path());
        events.extend([
            Event::TurnStart { turn: 0 },
            Event::RecoveryCheckpoint {
                state: checkpoint(),
            },
        ]);
        write_trace(&trace, events);

        let error = verify_trace(&trace).unwrap_err();
        assert!(error.contains("inside an active turn"), "{error}");
    }

    #[test]
    fn accepts_a_retryable_modern_crash_before_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("retryable-modern.jsonl");
        let mut events = prefix(dir.path());
        events.extend([
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
                calls: vec![modern_call("read-1", "read_file")],
            },
        ]);
        write_trace(&trace, events);

        let summary = verify_trace(&trace).unwrap();
        assert_eq!(summary.turns, 1);
        assert!(summary.stop_reason.is_none());
    }

    #[test]
    fn validates_a_complete_tool_transcript_without_executing_it() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("trace.jsonl");
        let would_be_written = dir.path().join("must-not-exist.txt");
        let mut events = prefix(dir.path());
        events.extend([
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(5),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ToolCall {
                id: "write-1".to_string(),
                name: "write_file".to_string(),
                args: json!({"path": "must-not-exist.txt", "content": "unsafe"}),
            },
            Event::ToolResult {
                id: "write-1".to_string(),
                name: "write_file".to_string(),
                output: "recorded only".to_string(),
                is_error: false,
                duration_ms: 1,
            },
            Event::TurnStart { turn: 1 },
            Event::TurnEnd {
                turn: 1,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(5),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ToolCall {
                id: "done-1".to_string(),
                name: ferric_loop::TASK_COMPLETE.to_string(),
                args: json!({"summary": "done"}),
            },
            Event::SessionEnd {
                reason: "task_complete".to_string(),
            },
        ]);
        write_trace(&trace, events);

        let summary = verify_trace(&trace).unwrap();
        assert_eq!(summary.turns, 2);
        assert_eq!(summary.tool_calls, 2);
        assert!(
            !would_be_written.exists(),
            "verification must never dispatch trace-authored tools"
        );
    }

    #[test]
    fn rejects_a_nonterminal_call_without_a_result() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("trace.jsonl");
        let mut events = prefix(dir.path());
        events.extend([
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: None,
                output_tokens: None,
                truncated: false,
            },
            Event::ToolCall {
                id: "read-1".to_string(),
                name: "read_file".to_string(),
                args: json!({"path": "missing.txt"}),
            },
            Event::SessionEnd {
                reason: "max_turns".to_string(),
            },
        ]);
        write_trace(&trace, events);

        let error = verify_trace(&trace).unwrap_err();
        assert!(error.contains("has no tool_result"), "{error}");
    }

    #[test]
    fn rejects_a_completed_native_turn_with_missing_recorded_calls() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("trace.jsonl");
        let mut events = prefix(dir.path());
        events.extend([
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 2,
                input_tokens: Some(5),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::ToolCall {
                id: "done-1".to_string(),
                name: ferric_loop::TASK_COMPLETE.to_string(),
                args: json!({"summary": "incomplete recording"}),
            },
            Event::SessionEnd {
                reason: "task_complete".to_string(),
            },
        ]);
        write_trace(&trace, events);

        let error = verify_trace(&trace).unwrap_err();
        assert!(
            error.contains("declares 2 tool call(s), but 1 were recorded"),
            "{error}"
        );
    }

    #[test]
    fn accepts_a_native_turn_stopped_by_a_pre_dispatch_guard() {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("trace.jsonl");
        let mut events = prefix(dir.path());
        events.extend([
            Event::TurnStart { turn: 0 },
            Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: Some(5),
                output_tokens: Some(5),
                truncated: false,
            },
            Event::RepetitionGuard {
                action: "stopped".to_string(),
            },
            Event::SessionEnd {
                reason: "repetition_guard".to_string(),
            },
        ]);
        write_trace(&trace, events);

        let summary = verify_trace(&trace).unwrap();
        assert_eq!(summary.stop_reason.as_deref(), Some("repetition_guard"));
        assert_eq!(summary.tool_calls, 0);
    }

    #[test]
    fn accepts_a_real_provider_error_trace_without_turn_end() {
        use ferric_core::{ModelProfile, policy_for};
        use ferric_guard::{Provenance, SinkPolicy, Workspace};
        use ferric_loop::{RunArgs, StopReason, ThreadSleeper};
        use ferric_provider::SamplingParams;
        use ferric_tools::{Registry, register_builtin_tools};

        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("provider-error.jsonl");
        let workspace = Workspace::new(dir.path()).unwrap();
        let mut registry = Registry::new();
        register_builtin_tools(&mut registry);
        let provider = ferric_provider::MockProvider::new(Vec::new());
        let profile = ModelProfile {
            params_b: 1.2,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "mock".to_string(),
            measured_level: None,
        };
        let policy = policy_for(&profile);
        let mut sink = JsonlSink::open(&trace, "provider-error").unwrap();
        let args = RunArgs {
            edit_approver: None,
            cancel_flag: None,
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &policy,
            protocol: ActionProtocol::NativeTools,
            sampling: SamplingParams::default(),
            sleeper: &ThreadSleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            provenance: Provenance::Clean,
            sink_policy: SinkPolicy::deny(),
            hooks: None,
        };

        let outcome = futures_executor::block_on(ferric_loop::run(
            args,
            &mut sink,
            Some("fail before completing"),
        ))
        .unwrap();
        assert_eq!(outcome.stop, StopReason::ProviderError);
        drop(sink);

        let summary = verify_trace(&trace).unwrap();
        assert_eq!(summary.stop_reason.as_deref(), Some("provider_error"));
        assert_eq!(summary.turns, 1);
    }
}
