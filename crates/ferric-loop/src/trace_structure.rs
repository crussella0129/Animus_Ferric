//! Shared structural validation for recovery-aware traces.
//!
//! The projector deliberately has no error channel: it is also used while a
//! live run is writing known-good events. Traces read from disk are untrusted,
//! however, so replay and `trace verify` both feed events through this state
//! machine before projecting them. Keeping the additive recovery protocol in
//! one validator prevents a verifier from blessing a sequence that replay
//! interprets differently.

use std::collections::{BTreeMap, BTreeSet};

use ferric_core::{HarnessPolicy, ToolCall};
use ferric_trace::{Event, RecoveryCheckpointV1};

use crate::terminator::{REQUEST_USER_INPUT, SUBMIT_PLAN, TASK_COMPLETE};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum StateBase {
    #[default]
    None,
    Fresh,
    Resumed,
}

#[derive(Debug)]
struct RecordedCall {
    call: ToolCall,
    result: Option<bool>,
    mutation_recorded: bool,
    check_recorded: Option<String>,
}

#[derive(Debug)]
struct ActiveTurn {
    turn: u32,
    ended: bool,
    proposed: Option<Vec<ToolCall>>,
    calls: Vec<RecordedCall>,
    pre_dispatch_stopped: bool,
}

impl ActiveTurn {
    fn new(turn: u32) -> Self {
        Self {
            turn,
            ended: false,
            proposed: None,
            calls: Vec::new(),
            pre_dispatch_stopped: false,
        }
    }

    fn is_modern(&self) -> bool {
        self.proposed.is_some()
    }
}

/// Validates the ordering and cross-event identities introduced by the
/// recovery protocol. Legacy turns (those without `ActionsProposed`) retain
/// their historical next-`TurnStart` boundary and are left to the caller's
/// existing legacy checks.
#[derive(Debug, Default)]
pub struct TraceStructure {
    base: StateBase,
    active: Option<ActiveTurn>,
    saw_turn: bool,
    last_turn: Option<u32>,
    expected_first_turn: Option<u32>,
    resume_prompt_seen: bool,
    answer_anchor_seen: bool,
    ended_reason: Option<String>,
    checkpoint_after_end: bool,
    paused: bool,
    awaiting_end: Option<String>,
    committed_terminal: Option<String>,
    mutation_epoch: u64,
    passed_checks: BTreeMap<String, u64>,
    harness_policy: Option<HarnessPolicy>,
}

impl TraceStructure {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, event: &Event) -> Result<(), String> {
        if self.ended_reason.is_some()
            && !matches!(
                event,
                Event::Note { .. } | Event::RecoveryCheckpoint { .. } | Event::SessionPaused { .. }
            )
        {
            return Err(format!("event appears after SessionEnd: {event:?}"));
        }

        match event {
            Event::PolicySelected { harness_policy, .. } => {
                self.observe_policy_selected(*harness_policy)
            }
            Event::SessionPrompt { .. } => self.observe_session_prompt(),
            Event::RecoveryCheckpoint { state } => self.observe_checkpoint(state),
            Event::ResumePrompt { .. } => self.observe_resume_prompt(),
            Event::TurnStart { turn } => self.observe_turn_start(*turn),
            Event::TurnEnd { turn, .. } => self.observe_turn_end(*turn),
            Event::ActionsProposed { turn, calls } => self.observe_actions_proposed(*turn, calls),
            Event::ToolCall { id, name, args } => self.observe_tool_call(ToolCall {
                id: id.clone(),
                name: name.clone(),
                args: args.clone(),
            }),
            Event::ToolResult {
                id, name, is_error, ..
            } => self.observe_tool_result(id, name, *is_error),
            Event::ObservationRecorded { .. }
            | Event::ControllerBlocked { .. }
            | Event::WorkspaceEffectRecorded { .. }
            | Event::VerificationCheckRecorded { .. }
            | Event::ControllerCheckpoint { .. }
            | Event::RecoveryPacketInjected { .. } => {
                self.reject_unintegrated_controller_event(event)
            }
            Event::WorkspaceMutation {
                turn,
                tool,
                mutation_epoch,
            } => self.observe_mutation(*turn, tool, *mutation_epoch),
            Event::VerificationCheckPassed {
                turn,
                name,
                mutation_epoch,
            } => self.observe_check(*turn, name, *mutation_epoch),
            Event::CompletionGate {
                mutation_epoch,
                required_checks,
                fresh_checks,
                decision,
            } => self.observe_gate(*mutation_epoch, required_checks, fresh_checks, decision),
            Event::RepetitionGuard { action }
            | Event::NoProgressGuard { action }
            | Event::OscillationGuard { action }
                if action == "stopped" =>
            {
                let active = self.active_mut("guard stop")?;
                if !active.ended {
                    return Err("guard stopped before TurnEnd".to_string());
                }
                if active.pre_dispatch_stopped {
                    return Err("more than one pre-dispatch guard stopped a turn".to_string());
                }
                active.pre_dispatch_stopped = true;
                Ok(())
            }
            Event::TurnCommitted {
                turn,
                dispatched,
                errored,
                stop_reason,
                ..
            } => self.observe_commit(*turn, *dispatched, *errored, stop_reason.as_deref()),
            Event::SessionEnd { reason } => self.observe_session_end(reason),
            Event::SessionPaused { reason } => self.observe_session_paused(reason),
            _ => Ok(()),
        }
    }

    fn observe_policy_selected(&mut self, policy: HarnessPolicy) -> Result<(), String> {
        if self.harness_policy.is_some() {
            return Err("trace contains more than one PolicySelected".to_string());
        }
        if self.saw_turn || self.base != StateBase::None {
            return Err("PolicySelected appears after session state began".to_string());
        }
        self.harness_policy = Some(policy);
        Ok(())
    }

    /// B113-01 makes the new wire vocabulary readable but deliberately emits
    /// none of it. Treating a newly-known event as harmless via the wildcard
    /// would let a forged evidence trace pass `trace verify` before the causal
    /// controller state machine exists. Later build units replace this gate
    /// with full per-event validation; until then both legacy misuse and
    /// prematurely-authored evidence traces fail closed here.
    fn reject_unintegrated_controller_event(&self, event: &Event) -> Result<(), String> {
        match self.harness_policy {
            Some(HarnessPolicy::Legacy) | None => Err(format!(
                "controller event is not valid under the legacy harness policy: {event:?}"
            )),
            Some(HarnessPolicy::Evidence | HarnessPolicy::EvidencePlanner) => Err(format!(
                "controller event validation is not enabled yet: {event:?}"
            )),
        }
    }

    /// A terminal commit can be durable even when the process crashes before
    /// writing `SessionEnd`. Replay uses this to fail closed on successful
    /// completion and to retain a non-success pause reason.
    pub fn unclosed_terminal_reason(&self) -> Option<&str> {
        self.awaiting_end.as_deref()
    }

    pub fn committed_terminal_reason(&self) -> Option<&str> {
        self.committed_terminal.as_deref()
    }

    /// EOF is a valid crash boundary. An open modern proposal is either a safe
    /// pre-dispatch retry or is classified more precisely by replay's
    /// ambiguity check; no state transition is performed here.
    pub fn finish(&self) -> Result<(), String> {
        if self.base == StateBase::Resumed && self.resume_prompt_seen && !self.answer_anchor_seen {
            // Crash after ResumePrompt and before its anchor is intentional:
            // the projector applies the answer and clears pending input when
            // it sees ResumePrompt, making this prefix resume-of-resume safe.
            return Ok(());
        }
        Ok(())
    }

    fn observe_session_prompt(&mut self) -> Result<(), String> {
        if self.base != StateBase::None || self.saw_turn {
            return Err("SessionPrompt conflicts with an existing state base".to_string());
        }
        self.base = StateBase::Fresh;
        self.expected_first_turn = Some(0);
        Ok(())
    }

    fn observe_checkpoint(&mut self, state: &RecoveryCheckpointV1) -> Result<(), String> {
        if self.active.is_some() {
            return Err("RecoveryCheckpoint appears inside an active turn".to_string());
        }
        if state.head_len > state.messages.len() {
            return Err("checkpoint head_len exceeds message count".to_string());
        }
        if state
            .passed_checks
            .values()
            .any(|epoch| *epoch > state.mutation_epoch)
        {
            return Err("checkpoint contains check evidence from a future epoch".to_string());
        }

        if let Some(reason) = self.ended_reason.as_deref() {
            if is_success_reason(reason) {
                return Err("successful sessions cannot carry a recovery checkpoint".to_string());
            }
            if self.checkpoint_after_end || self.paused {
                return Err("duplicate or late recovery checkpoint after SessionEnd".to_string());
            }
            self.checkpoint_after_end = true;
            self.import_checkpoint(state);
            return Ok(());
        }

        if self.saw_turn {
            return Err("RecoveryCheckpoint appears after turns without SessionEnd".to_string());
        }
        match self.base {
            StateBase::None => {
                self.base = StateBase::Resumed;
                self.expected_first_turn = Some(state.next_turn);
                self.import_checkpoint(state);
                Ok(())
            }
            StateBase::Fresh => {
                Err("fresh SessionPrompt cannot be mixed with a recovery checkpoint".to_string())
            }
            StateBase::Resumed if self.resume_prompt_seen && !self.answer_anchor_seen => {
                if Some(state.next_turn) != self.expected_first_turn {
                    return Err("answer anchor changes the checkpoint next_turn".to_string());
                }
                if state.pending_input.is_some() {
                    return Err("answer anchor retains a pending input request".to_string());
                }
                self.answer_anchor_seen = true;
                self.import_checkpoint(state);
                Ok(())
            }
            StateBase::Resumed => Err("duplicate recovery state base".to_string()),
        }
    }

    fn import_checkpoint(&mut self, state: &RecoveryCheckpointV1) {
        self.mutation_epoch = state.mutation_epoch;
        self.passed_checks = state.passed_checks.clone();
    }

    fn observe_resume_prompt(&mut self) -> Result<(), String> {
        if self.base != StateBase::Resumed || self.saw_turn || self.resume_prompt_seen {
            return Err(
                "ResumePrompt requires one recovery base before the first turn".to_string(),
            );
        }
        self.resume_prompt_seen = true;
        Ok(())
    }

    fn observe_turn_start(&mut self, turn: u32) -> Result<(), String> {
        if let Some(reason) = &self.awaiting_end {
            return Err(format!(
                "TurnStart({turn}) appears after terminal TurnCommitted({reason})"
            ));
        }
        if self.base == StateBase::None {
            return Err("TurnStart appears before SessionPrompt or RecoveryCheckpoint".to_string());
        }
        if self.base == StateBase::Resumed && self.resume_prompt_seen && !self.answer_anchor_seen {
            return Err("TurnStart appears before the ResumePrompt answer anchor".to_string());
        }
        if let Some(active) = self.active.take()
            && active.is_modern()
        {
            return Err(format!(
                "TurnStart({turn}) overwrites modern turn {} without TurnCommitted",
                active.turn
            ));
        }

        let expected = if self.saw_turn {
            self.last_turn.map(|prior| prior.saturating_add(1))
        } else {
            self.expected_first_turn
        };
        if let Some(expected) = expected
            && turn != expected
        {
            return Err(format!("TurnStart({turn}) expected TurnStart({expected})"));
        }

        self.saw_turn = true;
        self.last_turn = Some(turn);
        self.active = Some(ActiveTurn::new(turn));
        Ok(())
    }

    fn observe_turn_end(&mut self, turn: u32) -> Result<(), String> {
        let active = self.active_mut("TurnEnd")?;
        if active.turn != turn {
            return Err(format!(
                "TurnEnd({turn}) does not match active turn {}",
                active.turn
            ));
        }
        if active.ended {
            return Err(format!("turn {turn} has more than one TurnEnd"));
        }
        active.ended = true;
        Ok(())
    }

    fn observe_actions_proposed(&mut self, turn: u32, calls: &[ToolCall]) -> Result<(), String> {
        let active = self.active_mut("ActionsProposed")?;
        if active.turn != turn || !active.ended {
            return Err(format!(
                "ActionsProposed({turn}) appears outside its ended turn"
            ));
        }
        if active.proposed.is_some() {
            return Err(format!("turn {turn} has duplicate ActionsProposed"));
        }
        if !active.calls.is_empty() {
            return Err(format!(
                "turn {turn} records ToolCall before ActionsProposed"
            ));
        }
        active.proposed = Some(calls.to_vec());
        Ok(())
    }

    fn observe_tool_call(&mut self, call: ToolCall) -> Result<(), String> {
        let active = self.active_mut("ToolCall")?;
        if !active.ended {
            return Err(format!("ToolCall({}) appears before TurnEnd", call.id));
        }
        if active
            .calls
            .iter()
            .any(|recorded| recorded.call.id == call.id)
        {
            return Err(format!("duplicate ToolCall id {:?}", call.id));
        }
        if let Some(previous) = active.calls.last()
            && previous.result.is_none()
            && !is_completion_control(&previous.call.name)
        {
            return Err(format!(
                "ToolCall({}) appears before ToolCall({}) received a result",
                call.id, previous.call.id
            ));
        }
        if let Some(proposed) = &active.proposed {
            let index = active.calls.len();
            let expected = proposed.get(index).ok_or_else(|| {
                format!(
                    "turn {} dispatches more calls than it proposed",
                    active.turn
                )
            })?;
            if expected != &call {
                return Err(format!(
                    "turn {} dispatched call {:?} but proposed {:?} at index {index}",
                    active.turn, call, expected
                ));
            }
        }
        active.calls.push(RecordedCall {
            call,
            result: None,
            mutation_recorded: false,
            check_recorded: None,
        });
        Ok(())
    }

    fn observe_tool_result(&mut self, id: &str, name: &str, is_error: bool) -> Result<(), String> {
        let active = self.active_mut("ToolResult")?;
        let recorded = active
            .calls
            .iter_mut()
            .find(|recorded| recorded.call.id == id)
            .ok_or_else(|| format!("ToolResult({id}) has no matching ToolCall"))?;
        if recorded.call.name != name {
            return Err(format!(
                "ToolResult({id}) name {name:?} does not match {:?}",
                recorded.call.name
            ));
        }
        if recorded.result.is_some() {
            return Err(format!("ToolCall({id}) has more than one result"));
        }
        if is_error && (recorded.mutation_recorded || recorded.check_recorded.is_some()) {
            return Err(format!(
                "failed ToolCall({id}) carries successful mutation/check evidence"
            ));
        }
        recorded.result = Some(is_error);
        Ok(())
    }

    fn observe_mutation(
        &mut self,
        turn: u32,
        tool: &str,
        mutation_epoch: u64,
    ) -> Result<(), String> {
        let expected_epoch = self.mutation_epoch.saturating_add(1);
        let active = self.active_mut("WorkspaceMutation")?;
        if active.turn != turn || !active.ended {
            return Err(format!(
                "WorkspaceMutation({turn}) appears outside its ended turn"
            ));
        }
        let recorded = active
            .calls
            .last_mut()
            .ok_or_else(|| "WorkspaceMutation has no matching ToolCall".to_string())?;
        if recorded.call.name != tool || recorded.result.is_some() || recorded.mutation_recorded {
            return Err(format!(
                "WorkspaceMutation({tool}) does not match the active unresolved call"
            ));
        }
        if mutation_epoch != expected_epoch {
            return Err(format!(
                "WorkspaceMutation advances epoch from {} to {mutation_epoch}",
                self.mutation_epoch
            ));
        }
        recorded.mutation_recorded = true;
        self.mutation_epoch = mutation_epoch;
        Ok(())
    }

    fn observe_check(&mut self, turn: u32, name: &str, mutation_epoch: u64) -> Result<(), String> {
        let current_epoch = self.mutation_epoch;
        let active = self.active_mut("VerificationCheckPassed")?;
        if active.turn != turn || !active.ended {
            return Err(format!(
                "VerificationCheckPassed({turn}) appears outside its ended turn"
            ));
        }
        let recorded = active
            .calls
            .last_mut()
            .ok_or_else(|| "VerificationCheckPassed has no matching ToolCall".to_string())?;
        let called_name = recorded
            .call
            .args
            .get("name")
            .and_then(serde_json::Value::as_str);
        if recorded.call.name != "run_check"
            || called_name != Some(name)
            || recorded.result.is_some()
            || recorded.check_recorded.is_some()
        {
            return Err(format!(
                "VerificationCheckPassed({name}) does not match the active run_check call"
            ));
        }
        if mutation_epoch != current_epoch {
            return Err(format!(
                "VerificationCheckPassed({name}) uses epoch {mutation_epoch}, current epoch is {current_epoch}"
            ));
        }
        recorded.check_recorded = Some(name.to_string());
        self.passed_checks.insert(name.to_string(), mutation_epoch);
        Ok(())
    }

    fn observe_gate(
        &mut self,
        mutation_epoch: u64,
        required_checks: &[String],
        fresh_checks: &[String],
        decision: &str,
    ) -> Result<(), String> {
        let active = self.active_mut("CompletionGate")?;
        if !active.ended {
            return Err("CompletionGate appears before TurnEnd".to_string());
        }
        if mutation_epoch != self.mutation_epoch {
            return Err(format!(
                "CompletionGate uses epoch {mutation_epoch}, current epoch is {}",
                self.mutation_epoch
            ));
        }
        let unique: BTreeSet<&str> = required_checks.iter().map(String::as_str).collect();
        if unique.len() != required_checks.len() {
            return Err("CompletionGate repeats a required check name".to_string());
        }
        let expected_fresh: Vec<String> = required_checks
            .iter()
            .filter(|name| self.passed_checks.get(*name) == Some(&self.mutation_epoch))
            .cloned()
            .collect();
        if fresh_checks != expected_fresh {
            return Err(format!(
                "CompletionGate fresh checks {fresh_checks:?} do not match recorded evidence {expected_fresh:?}"
            ));
        }
        let expected_decision = if expected_fresh.len() == required_checks.len() {
            "passed"
        } else {
            "blocked"
        };
        if decision != expected_decision {
            return Err(format!(
                "CompletionGate decision {decision:?} should be {expected_decision:?}"
            ));
        }
        Ok(())
    }

    fn observe_commit(
        &mut self,
        turn: u32,
        dispatched: u32,
        errored: u32,
        stop_reason: Option<&str>,
    ) -> Result<(), String> {
        if self.awaiting_end.is_some() {
            return Err("a terminal turn was already committed".to_string());
        }
        let active = self
            .active
            .take()
            .ok_or_else(|| format!("TurnCommitted({turn}) has no active turn"))?;
        if active.turn != turn || !active.ended {
            return Err(format!(
                "TurnCommitted({turn}) does not match an ended active turn"
            ));
        }
        let proposed = active
            .proposed
            .as_ref()
            .ok_or_else(|| format!("TurnCommitted({turn}) has no ActionsProposed"))?;
        if active.pre_dispatch_stopped {
            if !active.calls.is_empty() {
                return Err(format!(
                    "guard-stopped turn {turn} records dispatched calls"
                ));
            }
        } else if active.calls.len() != proposed.len() {
            return Err(format!(
                "turn {turn} committed {} call(s) after proposing {}",
                active.calls.len(),
                proposed.len()
            ));
        }

        for recorded in &active.calls {
            if recorded.result.is_none()
                && (!is_completion_control(&recorded.call.name)
                    || recorded.call.name == REQUEST_USER_INPUT)
            {
                return Err(format!(
                    "TurnCommitted({turn}) leaves ToolCall({}) unresolved",
                    recorded.call.id
                ));
            }
        }
        let result_count = active
            .calls
            .iter()
            .filter(|recorded| recorded.result.is_some())
            .count() as u32;
        let error_count = active
            .calls
            .iter()
            .filter(|recorded| recorded.result == Some(true))
            .count() as u32;
        if dispatched != result_count || errored != error_count {
            return Err(format!(
                "TurnCommitted({turn}) reports {dispatched}/{errored} dispatched/errors, recorded {result_count}/{error_count}"
            ));
        }

        if stop_reason == Some("needs_input")
            && (proposed.len() != 1
                || proposed[0].name != REQUEST_USER_INPUT
                || active.calls.len() != 1
                || active.calls[0].result != Some(false))
        {
            return Err(
                "needs_input commit lacks one successful request_user_input result".to_string(),
            );
        }
        if stop_reason == Some("task_complete")
            && !proposed.iter().any(|call| call.name == TASK_COMPLETE)
        {
            return Err("task_complete commit lacks task_complete proposal".to_string());
        }
        if stop_reason == Some("plan_submitted")
            && !proposed.iter().any(|call| call.name == SUBMIT_PLAN)
        {
            return Err("plan_submitted commit lacks submit_plan proposal".to_string());
        }
        if stop_reason == Some("final_text") && !proposed.is_empty() {
            return Err("final_text commit contains proposed actions".to_string());
        }

        self.last_turn = Some(turn);
        if let Some(reason) = stop_reason {
            self.awaiting_end = Some(reason.to_string());
            self.committed_terminal = Some(reason.to_string());
        }
        Ok(())
    }

    fn observe_session_end(&mut self, reason: &str) -> Result<(), String> {
        if self.ended_reason.is_some() {
            return Err("trace contains more than one SessionEnd".to_string());
        }
        if let Some(active) = self.active.take()
            && active.is_modern()
            && !(reason == "provider_error" && !active.ended && active.calls.is_empty())
        {
            return Err(format!(
                "SessionEnd({reason}) closes modern turn {} without TurnCommitted",
                active.turn
            ));
        }
        if let Some(committed) = self.awaiting_end.take()
            && committed != reason
        {
            return Err(format!(
                "terminal TurnCommitted({committed}) differs from SessionEnd({reason})"
            ));
        }
        self.ended_reason = Some(reason.to_string());
        Ok(())
    }

    fn observe_session_paused(&mut self, reason: &str) -> Result<(), String> {
        let ended = self
            .ended_reason
            .as_deref()
            .ok_or_else(|| "SessionPaused appears before SessionEnd".to_string())?;
        if is_success_reason(ended) {
            return Err("successful sessions cannot be paused".to_string());
        }
        if ended != reason {
            return Err(format!(
                "SessionPaused({reason}) differs from SessionEnd({ended})"
            ));
        }
        if !self.checkpoint_after_end {
            return Err("SessionPaused lacks a preceding recovery checkpoint".to_string());
        }
        if self.paused {
            return Err("trace contains more than one SessionPaused".to_string());
        }
        self.paused = true;
        Ok(())
    }

    fn active_mut(&mut self, event: &str) -> Result<&mut ActiveTurn, String> {
        self.active
            .as_mut()
            .ok_or_else(|| format!("{event} appears outside an active turn"))
    }
}

fn is_completion_control(name: &str) -> bool {
    matches!(name, TASK_COMPLETE | SUBMIT_PLAN)
}

fn is_success_reason(reason: &str) -> bool {
    matches!(
        reason,
        "final_text" | "task_complete" | "plan_submitted" | "done"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            args: json!({"path": "a.txt"}),
        }
    }

    fn prefix(validator: &mut TraceStructure) {
        validator
            .observe(&Event::SessionPrompt {
                system: "system".to_string(),
                user: "task".to_string(),
                media: Vec::new(),
            })
            .unwrap();
        validator.observe(&Event::TurnStart { turn: 0 }).unwrap();
        validator
            .observe(&Event::TurnEnd {
                turn: 0,
                text: None,
                tool_call_count: 1,
                input_tokens: None,
                output_tokens: None,
                truncated: false,
            })
            .unwrap();
    }

    fn policy(harness_policy: HarnessPolicy) -> Event {
        Event::PolicySelected {
            tier: ferric_core::Tier::Nano,
            protocol: ferric_core::ActionProtocol::NativeTools,
            harness_policy,
            max_turns: 15,
            max_tools: 10,
            prompt_budget_tokens: 2_800,
            max_output_tokens: 512,
            truncation_limit: ferric_core::DEFAULT_TRUNCATION_LIMIT,
            tier_source: ferric_core::TierSource::Params.label().to_string(),
        }
    }

    fn observation_event() -> Event {
        Event::ObservationRecorded {
            turn: 0,
            call_id: "read-1".to_string(),
            observation: ferric_trace::ObservationV1 {
                version: ferric_trace::CONTROLLER_RECORD_VERSION,
                detail: ferric_trace::ObservationDetailV1::Find(
                    ferric_trace::NavigationObservationV1 {
                        root: ".".to_string(),
                        literal: "missing.rs".to_string(),
                        match_count: 0,
                        max_results: 100,
                        exhausted: true,
                        result_sha256: "0".repeat(64),
                    },
                ),
            },
        }
    }

    #[test]
    fn legacy_policy_rejects_known_controller_events_instead_of_ignoring_them() {
        let mut validator = TraceStructure::new();
        validator.observe(&policy(HarnessPolicy::Legacy)).unwrap();

        let error = validator.observe(&observation_event()).unwrap_err();
        assert!(error.contains("legacy harness policy"), "{error}");
    }

    #[test]
    fn evidence_events_fail_closed_until_the_controller_validator_lands() {
        let mut validator = TraceStructure::new();
        validator.observe(&policy(HarnessPolicy::Evidence)).unwrap();

        let error = validator.observe(&observation_event()).unwrap_err();
        assert!(error.contains("not enabled yet"), "{error}");
    }

    #[test]
    fn modern_turn_cannot_be_overwritten_without_commit() {
        let mut validator = TraceStructure::new();
        prefix(&mut validator);
        validator
            .observe(&Event::ActionsProposed {
                turn: 0,
                calls: vec![call("a", "write_file")],
            })
            .unwrap();

        let error = validator
            .observe(&Event::TurnStart { turn: 1 })
            .unwrap_err();
        assert!(error.contains("without TurnCommitted"), "{error}");
    }

    #[test]
    fn proposal_and_dispatch_must_match_exactly() {
        let mut validator = TraceStructure::new();
        prefix(&mut validator);
        validator
            .observe(&Event::ActionsProposed {
                turn: 0,
                calls: vec![call("a", "read_file")],
            })
            .unwrap();

        let error = validator
            .observe(&Event::ToolCall {
                id: "b".to_string(),
                name: "write_file".to_string(),
                args: json!({"path": "a.txt"}),
            })
            .unwrap_err();
        assert!(error.contains("dispatched call"), "{error}");
    }

    #[test]
    fn checkpoint_cannot_erase_an_active_turn() {
        let mut validator = TraceStructure::new();
        prefix(&mut validator);
        let checkpoint = RecoveryCheckpointV1 {
            version: ferric_trace::RECOVERY_CHECKPOINT_VERSION,
            messages: Vec::new(),
            next_turn: 0,
            last_text: None,
            head_len: 0,
            committed_turn_starts: Vec::new(),
            guard_history: Vec::new(),
            nudged_for_no_action: false,
            truncated_once: false,
            last_input_tokens: None,
            pending_input: None,
            mutation_epoch: 0,
            passed_checks: BTreeMap::new(),
        };

        let error = validator
            .observe(&Event::RecoveryCheckpoint { state: checkpoint })
            .unwrap_err();
        assert!(error.contains("inside an active turn"), "{error}");
    }
}
