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
use ferric_trace::{
    ControllerBlockReason, ControllerBlockV1, ControllerCheckpointV1, Event, ObservationDetailV1,
    ObservationV1, RecoveryCheckpointV1, RecoveryPacketV1, VerificationOutcome, WorkspaceEffectV1,
};

use crate::controller::ControllerState;
use crate::terminator::{REQUEST_USER_INPUT, SUBMIT_PLAN, TASK_COMPLETE};

pub(crate) fn validate_recovery_checkpoint_shape(
    state: &RecoveryCheckpointV1,
) -> Result<(), String> {
    if state.head_len > state.messages.len() {
        return Err("checkpoint head_len exceeds message count".to_string());
    }
    if let Some(request) = &state.pending_input {
        request
            .validate()
            .map_err(|error| format!("invalid pending input request: {error}"))?;
    }
    if state
        .passed_checks
        .values()
        .any(|epoch| *epoch > state.mutation_epoch)
    {
        return Err("checkpoint contains check evidence from a future epoch".to_string());
    }

    let mut prior_boundary: Option<(u32, usize)> = None;
    for boundary in &state.committed_turn_starts {
        if boundary.message_index < state.head_len || boundary.message_index >= state.messages.len()
        {
            return Err(format!(
                "checkpoint turn {} boundary {} is outside retained turn history",
                boundary.turn, boundary.message_index
            ));
        }
        if boundary.turn >= state.next_turn {
            return Err(format!(
                "checkpoint turn boundary {} is not before next_turn {}",
                boundary.turn, state.next_turn
            ));
        }
        if let Some((prior_turn, prior_index)) = prior_boundary
            && (boundary.turn <= prior_turn || boundary.message_index <= prior_index)
        {
            return Err(
                "checkpoint turn boundaries are not strictly increasing by turn and message index"
                    .to_string(),
            );
        }
        prior_boundary = Some((boundary.turn, boundary.message_index));
    }

    let retained_turns: BTreeSet<u32> = state
        .committed_turn_starts
        .iter()
        .map(|boundary| boundary.turn)
        .collect();
    let first_retained_turn = state
        .committed_turn_starts
        .first()
        .map(|boundary| boundary.turn);
    let mut prior_guard_turn = None;
    for guarded in &state.guard_history {
        if guarded.turn >= state.next_turn {
            return Err(format!(
                "checkpoint guard turn {} is not before next_turn {}",
                guarded.turn, state.next_turn
            ));
        }
        if prior_guard_turn.is_some_and(|prior| guarded.turn <= prior) {
            return Err("checkpoint guard turns are not strictly increasing".to_string());
        }
        if guarded.calls.is_empty() {
            return Err(format!(
                "checkpoint guard turn {} contains no proposed calls",
                guarded.turn
            ));
        }
        if guarded.dispatched as usize > guarded.calls.len() || guarded.errored > guarded.dispatched
        {
            return Err(format!(
                "checkpoint guard turn {} has incoherent dispatch/error counts",
                guarded.turn
            ));
        }
        if first_retained_turn.is_some_and(|first| guarded.turn >= first)
            && !retained_turns.contains(&guarded.turn)
        {
            return Err(format!(
                "checkpoint guard turn {} has no retained message boundary",
                guarded.turn
            ));
        }
        prior_guard_turn = Some(guarded.turn);
    }
    Ok(())
}

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
    controller_record: Option<ControllerCallRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ControllerCallRecord {
    Observation,
    Blocked(ControllerBlockReason),
    Effect,
    Verification {
        name: String,
        outcome: VerificationOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ControllerCheckpointContext {
    Initial,
    ResumeBase {
        mutation_epoch: u64,
        passed_checks: BTreeMap<String, u64>,
    },
    ResumeAnchor,
    Pause {
        reason: String,
    },
}

#[derive(Debug)]
struct ActiveTurn {
    turn: u32,
    ended: bool,
    proposed: Option<Vec<ToolCall>>,
    calls: Vec<RecordedCall>,
    pre_dispatch_stopped: bool,
    completion_gate: Option<CompletionGateRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompletionGateRecord {
    mutation_epoch: u64,
    passed: bool,
}

impl ActiveTurn {
    fn new(turn: u32) -> Self {
        Self {
            turn,
            ended: false,
            proposed: None,
            calls: Vec::new(),
            pre_dispatch_stopped: false,
            completion_gate: None,
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
    controller: Option<ControllerState>,
    pending_controller_checkpoint: Option<ControllerCheckpointContext>,
    recovery_packet_seen: bool,
    resumed_pending_input: bool,
}

impl TraceStructure {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, event: &Event) -> Result<(), String> {
        if self.pending_controller_checkpoint.is_some()
            && !matches!(event, Event::ControllerCheckpoint { .. })
        {
            return Err(format!(
                "controller checkpoint must immediately follow its state anchor, got {event:?}"
            ));
        }
        if self.ended_reason.is_some()
            && !matches!(
                event,
                Event::Note { .. }
                    | Event::RecoveryCheckpoint { .. }
                    | Event::ControllerCheckpoint { .. }
                    | Event::SessionPaused { .. }
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
            Event::ObservationRecorded {
                turn,
                call_id,
                observation,
            } => self.observe_controller_observation(*turn, call_id, observation),
            Event::ControllerBlocked {
                turn,
                call_id,
                tool,
                block,
            } => self.observe_controller_block(*turn, call_id, tool, block),
            Event::WorkspaceEffectRecorded {
                turn,
                call_id,
                tool,
                effect,
            } => self.observe_workspace_effect(*turn, call_id, tool, effect),
            Event::VerificationCheckRecorded {
                turn,
                call_id,
                check,
            } => self.observe_verification_check(*turn, call_id, check),
            Event::ControllerCheckpoint { state } => self.observe_controller_checkpoint(state),
            Event::RecoveryPacketInjected { packet, message } => {
                self.observe_recovery_packet(packet, message)
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
        if policy == HarnessPolicy::EvidencePlanner {
            return Err(
                "evidence_planner is unsupported until a planner trace protocol is defined"
                    .to_string(),
            );
        }
        self.harness_policy = Some(policy);
        Ok(())
    }

    fn ensure_evidence_harness(&self, event: &str) -> Result<(), String> {
        match self.harness_policy {
            Some(HarnessPolicy::Legacy) | None => Err(format!(
                "{event} is not valid under the legacy harness policy"
            )),
            Some(HarnessPolicy::Evidence) => Ok(()),
            Some(HarnessPolicy::EvidencePlanner) => Err(
                "evidence_planner is unsupported until a planner trace protocol is defined"
                    .to_string(),
            ),
        }
    }

    fn observe_controller_observation(
        &mut self,
        turn: u32,
        call_id: &str,
        observation: &ObservationV1,
    ) -> Result<(), String> {
        self.ensure_evidence_harness("ObservationRecorded")?;
        let recorded = self.controller_call("ObservationRecorded", turn, call_id, None)?;
        validate_observation_matches_call(&recorded.call, observation)
            .map_err(|error| format!("ObservationRecorded({call_id}) {error}"))?;
        self.controller
            .as_mut()
            .ok_or_else(|| "ObservationRecorded appears before controller base".to_string())?
            .apply_observation(turn, observation)
            .map_err(|error| format!("invalid ObservationRecorded({call_id}): {error}"))?;
        self.set_controller_call_record("ObservationRecorded", ControllerCallRecord::Observation)
    }

    fn observe_controller_block(
        &mut self,
        turn: u32,
        call_id: &str,
        tool: &str,
        block: &ControllerBlockV1,
    ) -> Result<(), String> {
        self.ensure_evidence_harness("ControllerBlocked")?;
        let recorded = self.controller_call("ControllerBlocked", turn, call_id, Some(tool))?;
        validate_block_paths_match_call(&recorded.call, block)
            .map_err(|error| format!("ControllerBlocked({call_id}) {error}"))?;
        if block.reason == ControllerBlockReason::RepeatedCheck
            || (block.reason == ControllerBlockReason::UnsupportedMutation
                && block.check_name.is_some())
        {
            if tool != "run_check" {
                return Err(format!(
                    "check ControllerBlocked({call_id}) does not match run_check"
                ));
            }
            let called_name = recorded
                .call
                .args
                .get("name")
                .and_then(serde_json::Value::as_str);
            if called_name != block.check_name.as_deref() {
                return Err(format!(
                    "ControllerBlocked({call_id}) check name does not match ToolCall args"
                ));
            }
        } else if block.check_name.is_some() {
            return Err(format!(
                "non-check ControllerBlocked({call_id}) carries a check name"
            ));
        }
        self.controller
            .as_ref()
            .ok_or_else(|| "ControllerBlocked appears before controller base".to_string())?
            .validate_block(turn, block)
            .map_err(|error| format!("invalid ControllerBlocked({call_id}): {error}"))?;
        self.set_controller_call_record(
            "ControllerBlocked",
            ControllerCallRecord::Blocked(block.reason),
        )
    }

    fn observe_workspace_effect(
        &mut self,
        turn: u32,
        call_id: &str,
        tool: &str,
        effect: &WorkspaceEffectV1,
    ) -> Result<(), String> {
        self.ensure_evidence_harness("WorkspaceEffectRecorded")?;
        if !is_single_target_content_tool(tool) {
            return Err(format!(
                "WorkspaceEffectRecorded({call_id}) uses unsupported effect tool {tool:?}"
            ));
        }
        let recorded =
            self.controller_call("WorkspaceEffectRecorded", turn, call_id, Some(tool))?;
        let effect_paths: Vec<String> = effect
            .effects
            .iter()
            .map(|path_effect| path_effect.path.clone())
            .collect();
        validate_effect_paths_match_call(&recorded.call, &effect_paths)
            .map_err(|error| format!("WorkspaceEffectRecorded({call_id}) {error}"))?;
        let controller = self
            .controller
            .as_mut()
            .ok_or_else(|| "WorkspaceEffectRecorded appears before controller base".to_string())?;
        controller
            .apply_workspace_effect(turn, effect)
            .map_err(|error| format!("invalid WorkspaceEffectRecorded({call_id}): {error}"))?;
        self.mutation_epoch = controller.mutation_epoch();
        self.passed_checks = controller.passed_checks().clone();
        self.set_controller_call_record("WorkspaceEffectRecorded", ControllerCallRecord::Effect)
    }

    fn observe_verification_check(
        &mut self,
        turn: u32,
        call_id: &str,
        check: &ferric_trace::VerificationCheckV1,
    ) -> Result<(), String> {
        self.ensure_evidence_harness("VerificationCheckRecorded")?;
        let recorded = self.controller_call(
            "VerificationCheckRecorded",
            turn,
            call_id,
            Some("run_check"),
        )?;
        let called_name = recorded
            .call
            .args
            .get("name")
            .and_then(serde_json::Value::as_str);
        if called_name != Some(check.name.as_str()) {
            return Err(format!(
                "VerificationCheckRecorded({call_id}) name does not match ToolCall args"
            ));
        }
        let controller = self.controller.as_mut().ok_or_else(|| {
            "VerificationCheckRecorded appears before controller base".to_string()
        })?;
        controller
            .apply_verification_check(turn, check)
            .map_err(|error| format!("invalid VerificationCheckRecorded({call_id}): {error}"))?;
        self.passed_checks = controller.passed_checks().clone();
        self.set_controller_call_record(
            "VerificationCheckRecorded",
            ControllerCallRecord::Verification {
                name: check.name.clone(),
                outcome: check.outcome,
            },
        )
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

    /// Canonical controller truth projected from every structurally validated
    /// event observed so far. Replay uses this single source instead of
    /// maintaining a second controller-event parser.
    pub(crate) fn controller_checkpoint(&self) -> Option<ControllerCheckpointV1> {
        self.controller.as_ref().map(ControllerState::checkpoint)
    }

    /// EOF is a valid crash boundary. An open modern proposal is either a safe
    /// pre-dispatch retry or is classified more precisely by replay's
    /// ambiguity check; no state transition is performed here.
    pub fn finish(&self) -> Result<(), String> {
        if self.pending_controller_checkpoint.is_some() {
            return Err("trace ends before its required ControllerCheckpoint".to_string());
        }
        if self.harness_policy == Some(HarnessPolicy::Evidence)
            && self.base != StateBase::None
            && self.controller.is_none()
        {
            return Err("evidence trace ends without a controller base".to_string());
        }
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
        if self.harness_policy == Some(HarnessPolicy::Evidence) {
            self.pending_controller_checkpoint = Some(ControllerCheckpointContext::Initial);
        }
        Ok(())
    }

    fn observe_checkpoint(&mut self, state: &RecoveryCheckpointV1) -> Result<(), String> {
        if self.active.is_some() {
            return Err("RecoveryCheckpoint appears inside an active turn".to_string());
        }
        validate_recovery_checkpoint_shape(state)?;

        if let Some(reason) = self.ended_reason.clone() {
            if is_success_reason(&reason) {
                return Err("successful sessions cannot carry a recovery checkpoint".to_string());
            }
            if self.checkpoint_after_end || self.paused {
                return Err("duplicate or late recovery checkpoint after SessionEnd".to_string());
            }
            self.checkpoint_after_end = true;
            if self.harness_policy == Some(HarnessPolicy::Evidence) {
                self.validate_core_controller_parity(state)?;
                self.pending_controller_checkpoint = Some(ControllerCheckpointContext::Pause {
                    reason: reason.clone(),
                });
            }
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
                if self.harness_policy == Some(HarnessPolicy::Evidence) {
                    self.resumed_pending_input = state.pending_input.is_some();
                    self.pending_controller_checkpoint =
                        Some(ControllerCheckpointContext::ResumeBase {
                            mutation_epoch: state.mutation_epoch,
                            passed_checks: state.passed_checks.clone(),
                        });
                }
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
                if self.harness_policy == Some(HarnessPolicy::Evidence) {
                    self.validate_core_controller_parity(state)?;
                    self.pending_controller_checkpoint =
                        Some(ControllerCheckpointContext::ResumeAnchor);
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

    fn validate_core_controller_parity(&self, state: &RecoveryCheckpointV1) -> Result<(), String> {
        let controller = self
            .controller
            .as_ref()
            .ok_or_else(|| "RecoveryCheckpoint appears before controller base".to_string())?;
        if state.mutation_epoch != controller.mutation_epoch()
            || state.passed_checks != *controller.passed_checks()
        {
            return Err(
                "RecoveryCheckpoint mutation/check coordinates disagree with controller state"
                    .to_string(),
            );
        }
        Ok(())
    }

    fn observe_controller_checkpoint(
        &mut self,
        state: &ControllerCheckpointV1,
    ) -> Result<(), String> {
        self.ensure_evidence_harness("ControllerCheckpoint")?;
        if self.active.is_some() {
            return Err("ControllerCheckpoint appears inside an active turn".to_string());
        }
        if state.harness_policy != HarnessPolicy::Evidence {
            return Err("ControllerCheckpoint policy differs from PolicySelected".to_string());
        }
        let context = self
            .pending_controller_checkpoint
            .take()
            .ok_or_else(|| "ControllerCheckpoint has no matching state anchor".to_string())?;
        let parsed = ControllerState::from_checkpoint(state)
            .map_err(|error| format!("invalid ControllerCheckpoint: {error}"))?;

        match context {
            ControllerCheckpointContext::Initial => {
                let pristine =
                    ControllerState::new(HarnessPolicy::Evidence, state.required_checks.clone())
                        .map_err(|error| format!("invalid initial ControllerCheckpoint: {error}"))?
                        .checkpoint();
                if *state != pristine {
                    return Err(
                        "initial ControllerCheckpoint is not a pristine controller base"
                            .to_string(),
                    );
                }
            }
            ControllerCheckpointContext::ResumeBase {
                mutation_epoch,
                passed_checks,
            } => {
                if state.mutation_epoch != mutation_epoch || state.passed_checks != passed_checks {
                    return Err(
                        "resumed ControllerCheckpoint disagrees with core recovery coordinates"
                            .to_string(),
                    );
                }
                if state.inherited_pause_reason.is_none()
                    || state.file_evidence.iter().any(|evidence| {
                        evidence.fresh || evidence.complete || !evidence.covered_ranges.is_empty()
                    })
                {
                    return Err(
                        "resumed ControllerCheckpoint did not conservatively invalidate inherited file coverage"
                            .to_string(),
                    );
                }
            }
            ControllerCheckpointContext::ResumeAnchor => {
                let expected = self
                    .controller
                    .as_ref()
                    .ok_or_else(|| "controller answer anchor has no resume base".to_string())?
                    .checkpoint();
                if *state != expected {
                    return Err(
                        "controller answer anchor differs from resumed controller state"
                            .to_string(),
                    );
                }
            }
            ControllerCheckpointContext::Pause { reason } => {
                let expected = self
                    .controller
                    .as_ref()
                    .ok_or_else(|| {
                        "pause ControllerCheckpoint has no controller state".to_string()
                    })?
                    .checkpoint_for_pause(&reason)
                    .map_err(|error| format!("invalid pause ControllerCheckpoint: {error}"))?;
                if *state != expected {
                    return Err(
                        "pause ControllerCheckpoint differs from projected controller state"
                            .to_string(),
                    );
                }
            }
        }

        self.mutation_epoch = parsed.mutation_epoch();
        self.passed_checks = parsed.passed_checks().clone();
        self.controller = Some(parsed);
        Ok(())
    }

    fn observe_recovery_packet(
        &mut self,
        packet: &RecoveryPacketV1,
        message: &str,
    ) -> Result<(), String> {
        self.ensure_evidence_harness("RecoveryPacketInjected")?;
        if self.base != StateBase::Resumed || self.saw_turn || self.active.is_some() {
            return Err(
                "RecoveryPacketInjected must appear in a resumed base before the first turn"
                    .to_string(),
            );
        }
        if self.resume_prompt_seen && !self.answer_anchor_seen {
            return Err(
                "RecoveryPacketInjected appears before the ResumePrompt state anchors".to_string(),
            );
        }
        if self.resumed_pending_input {
            return Err(
                "clarification-answer continuation cannot inject a generic recovery packet"
                    .to_string(),
            );
        }
        if self.recovery_packet_seen {
            return Err("trace contains more than one RecoveryPacketInjected".to_string());
        }
        let controller = self
            .controller
            .as_ref()
            .ok_or_else(|| "RecoveryPacketInjected appears before controller base".to_string())?;
        let reason = controller
            .checkpoint()
            .inherited_pause_reason
            .ok_or_else(|| "resumed controller state omits its pause reason".to_string())?;
        let expected = controller
            .recovery_packet(&reason)
            .map_err(|error| format!("invalid RecoveryPacketInjected: {error}"))?;
        if *packet != expected {
            return Err(
                "RecoveryPacketInjected facts differ from projected controller state".to_string(),
            );
        }
        let expected_message = ControllerState::render_recovery_packet(packet)
            .map_err(|error| format!("invalid RecoveryPacketInjected message: {error}"))?;
        if message != expected_message {
            return Err(
                "RecoveryPacketInjected message differs from its deterministic typed facts"
                    .to_string(),
            );
        }
        self.recovery_packet_seen = true;
        Ok(())
    }

    fn observe_resume_prompt(&mut self) -> Result<(), String> {
        if self.base != StateBase::Resumed || self.saw_turn || self.resume_prompt_seen {
            return Err(
                "ResumePrompt requires one recovery base before the first turn".to_string(),
            );
        }
        if self.recovery_packet_seen {
            return Err("ResumePrompt cannot appear after RecoveryPacketInjected".to_string());
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
        if self.base == StateBase::Resumed && self.resumed_pending_input && !self.resume_prompt_seen
        {
            return Err(
                "TurnStart appears before the pending clarification received a ResumePrompt"
                    .to_string(),
            );
        }
        if self.harness_policy == Some(HarnessPolicy::Evidence) {
            if self.controller.is_none() {
                return Err("TurnStart appears before the controller base".to_string());
            }
            if self.base == StateBase::Resumed
                && !self.resumed_pending_input
                && !self.recovery_packet_seen
            {
                return Err("resumed evidence TurnStart lacks RecoveryPacketInjected".to_string());
            }
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
            self.last_turn
                .map(|prior| {
                    prior
                        .checked_add(1)
                        .ok_or_else(|| "turn coordinate cannot advance beyond u32::MAX".to_string())
                })
                .transpose()?
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
            controller_record: None,
        });
        Ok(())
    }

    fn observe_tool_result(&mut self, id: &str, name: &str, is_error: bool) -> Result<(), String> {
        let evidence_policy = matches!(self.harness_policy, Some(HarnessPolicy::Evidence));
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
        let typed_effect_mirror = evidence_policy
            && recorded.mutation_recorded
            && recorded.controller_record == Some(ControllerCallRecord::Effect);
        if is_error
            && (recorded.mutation_recorded || recorded.check_recorded.is_some())
            && !typed_effect_mirror
        {
            return Err(format!(
                "failed ToolCall({id}) carries successful mutation/check evidence"
            ));
        }
        match recorded.controller_record.as_ref() {
            Some(ControllerCallRecord::Observation) if is_error => {
                return Err(format!(
                    "failed ToolCall({id}) carries a successful observation record"
                ));
            }
            Some(ControllerCallRecord::Blocked(_)) if !is_error => {
                return Err(format!(
                    "ControllerBlocked ToolCall({id}) has a successful ToolResult"
                ));
            }
            Some(ControllerCallRecord::Verification {
                name: check_name,
                outcome,
            }) => {
                let expected_error = *outcome == VerificationOutcome::Failed;
                if is_error != expected_error {
                    return Err(format!(
                        "ToolCall({id}) result disagrees with verification outcome {outcome:?}"
                    ));
                }
                match outcome {
                    VerificationOutcome::Passed
                        if recorded.check_recorded.as_deref() != Some(check_name) =>
                    {
                        return Err(format!(
                            "passing VerificationCheckRecorded({id}) lacks its compatibility VerificationCheckPassed"
                        ));
                    }
                    VerificationOutcome::Failed if recorded.check_recorded.is_some() => {
                        return Err(format!(
                            "failed VerificationCheckRecorded({id}) carries passing compatibility evidence"
                        ));
                    }
                    _ => {}
                }
            }
            // A measured partial effect may coexist with an errored result.
            Some(ControllerCallRecord::Effect) if !recorded.mutation_recorded => {
                return Err(format!(
                    "WorkspaceEffectRecorded({id}) lacks its compatibility WorkspaceMutation"
                ));
            }
            Some(ControllerCallRecord::Effect | ControllerCallRecord::Blocked(_))
            | Some(ControllerCallRecord::Observation)
            | None => {}
        }
        if evidence_policy
            && !is_error
            && recorded.controller_record.is_none()
            && !is_unmeasured_read_only_or_control(name)
        {
            return Err(format!(
                "successful evidence-policy ToolCall({id}) lacks a typed controller record"
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
        if self.harness_policy == Some(HarnessPolicy::Evidence) {
            let current_epoch = self.mutation_epoch;
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
            if recorded.call.name != tool
                || recorded.result.is_some()
                || recorded.mutation_recorded
                || recorded.controller_record != Some(ControllerCallRecord::Effect)
                || mutation_epoch != current_epoch
            {
                return Err(format!(
                    "evidence WorkspaceMutation({tool}) is not an exact mirror of the active typed effect"
                ));
            }
            recorded.mutation_recorded = true;
            return Ok(());
        }
        let expected_epoch = self
            .mutation_epoch
            .checked_add(1)
            .ok_or_else(|| "WorkspaceMutation epoch cannot advance beyond u64::MAX".to_string())?;
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
        if self.harness_policy == Some(HarnessPolicy::Evidence) {
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
            let expected = ControllerCallRecord::Verification {
                name: name.to_string(),
                outcome: VerificationOutcome::Passed,
            };
            if recorded.call.name != "run_check"
                || recorded.result.is_some()
                || recorded.check_recorded.is_some()
                || recorded.controller_record.as_ref() != Some(&expected)
                || mutation_epoch != current_epoch
            {
                return Err(format!(
                    "evidence VerificationCheckPassed({name}) is not an exact mirror of the active typed pass"
                ));
            }
            recorded.check_recorded = Some(name.to_string());
            return Ok(());
        }
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
        if active.completion_gate.is_some() {
            return Err("turn contains more than one CompletionGate".to_string());
        }
        if self.harness_policy == Some(HarnessPolicy::Evidence) {
            let controller = self
                .controller
                .as_ref()
                .ok_or_else(|| "CompletionGate appears before controller base".to_string())?;
            if required_checks != controller.required_checks() {
                return Err(
                    "CompletionGate required checks differ from controller configuration"
                        .to_string(),
                );
            }
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
        self.active_mut("CompletionGate")?.completion_gate = Some(CompletionGateRecord {
            mutation_epoch,
            passed: decision == "passed",
        });
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
        if self.harness_policy == Some(HarnessPolicy::Evidence)
            && matches!(stop_reason, Some("task_complete" | "final_text"))
        {
            let controller = self
                .controller
                .as_ref()
                .ok_or_else(|| "successful evidence commit has no controller state".to_string())?;
            if !controller.required_checks().is_empty()
                && !active
                    .completion_gate
                    .is_some_and(|gate| gate.passed && gate.mutation_epoch == self.mutation_epoch)
            {
                return Err(
                    "successful evidence commit lacks a current passed CompletionGate".to_string(),
                );
            }
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

    fn controller_call(
        &self,
        event: &str,
        turn: u32,
        call_id: &str,
        tool: Option<&str>,
    ) -> Result<&RecordedCall, String> {
        let active = self
            .active
            .as_ref()
            .ok_or_else(|| format!("{event} appears outside an active turn"))?;
        if active.turn != turn || !active.ended {
            return Err(format!("{event}({turn}) appears outside its ended turn"));
        }
        let recorded = active
            .calls
            .last()
            .ok_or_else(|| format!("{event} has no matching ToolCall"))?;
        if recorded.call.id != call_id || recorded.result.is_some() {
            return Err(format!(
                "{event}({call_id}) does not match the active unresolved ToolCall"
            ));
        }
        if tool.is_some_and(|tool| tool != recorded.call.name) {
            return Err(format!(
                "{event}({call_id}) tool {tool:?} does not match {:?}",
                recorded.call.name
            ));
        }
        if recorded.controller_record.is_some() {
            return Err(format!(
                "ToolCall({call_id}) has more than one controller record"
            ));
        }
        Ok(recorded)
    }

    fn set_controller_call_record(
        &mut self,
        event: &str,
        record: ControllerCallRecord,
    ) -> Result<(), String> {
        let active = self.active_mut(event)?;
        let call = active
            .calls
            .last_mut()
            .ok_or_else(|| format!("{event} has no matching ToolCall"))?;
        if call.controller_record.replace(record).is_some() {
            return Err(format!(
                "ToolCall({}) has more than one controller record",
                call.call.id
            ));
        }
        Ok(())
    }
}

fn validate_observation_matches_call(
    call: &ToolCall,
    observation: &ObservationV1,
) -> Result<(), String> {
    match &observation.detail {
        ObservationDetailV1::File(file) => {
            if call.name != "read_file" {
                return Err(format!(
                    "file detail requires read_file, active tool is {:?}",
                    call.name
                ));
            }
            let path = required_string_arg(call, "path")?;
            if normalized_call_path(path)? != file.path {
                return Err("path does not match ToolCall args".to_string());
            }
            let start = optional_u64_arg(call, "start_line")?;
            let end = optional_u64_arg(call, "end_line")?;
            let requested_matches = match (&file.requested_range, start, end) {
                (None, None, None) => true,
                (Some(range), expected_start, expected_end) => {
                    range.start == expected_start && range.end == expected_end
                }
                _ => false,
            };
            if !requested_matches {
                return Err("requested range does not match ToolCall args".to_string());
            }
        }
        ObservationDetailV1::Search(navigation) => {
            validate_navigation_matches_call(call, navigation, "search_files", "query", 50)?;
        }
        ObservationDetailV1::Find(navigation) => {
            validate_navigation_matches_call(call, navigation, "find_files", "pattern", 100)?;
        }
    }
    Ok(())
}

fn validate_navigation_matches_call(
    call: &ToolCall,
    observation: &ferric_trace::NavigationObservationV1,
    tool: &str,
    literal_arg: &str,
    default_limit: u64,
) -> Result<(), String> {
    if call.name != tool {
        return Err(format!(
            "navigation detail requires {tool}, active tool is {:?}",
            call.name
        ));
    }
    let literal = required_string_arg(call, literal_arg)?;
    if literal != observation.literal {
        return Err("literal query/pattern does not match ToolCall args".to_string());
    }
    let root = call
        .args
        .get("path")
        .map(|_| required_string_arg(call, "path"))
        .transpose()?
        .unwrap_or(".");
    if normalized_call_path(root)? != observation.root {
        return Err("navigation root does not match ToolCall args".to_string());
    }
    let limit = optional_u64_arg(call, "max_results")?.unwrap_or(default_limit);
    if limit != observation.max_results {
        return Err("navigation result cap does not match ToolCall args".to_string());
    }
    Ok(())
}

fn validate_effect_paths_match_call(call: &ToolCall, paths: &[String]) -> Result<(), String> {
    let Some(targets) = statically_known_call_targets(call)? else {
        return Ok(());
    };
    if is_single_target_content_tool(&call.name)
        && paths.iter().cloned().collect::<BTreeSet<_>>() != targets
    {
        return Err(format!(
            "effect paths {paths:?} do not exactly match ToolCall({}) target",
            call.id
        ));
    }
    for path in paths {
        if !targets.contains(path) {
            return Err(format!(
                "path {path:?} is not a target of ToolCall({})",
                call.id
            ));
        }
    }
    Ok(())
}

fn validate_block_paths_match_call(
    call: &ToolCall,
    block: &ControllerBlockV1,
) -> Result<(), String> {
    let Some(targets) = statically_known_call_targets(call)? else {
        return Ok(());
    };
    let paths: BTreeSet<String> = block.paths.iter().cloned().collect();
    let path_bearing = matches!(
        block.reason,
        ControllerBlockReason::BlindMutation
            | ControllerBlockReason::StaleObservation
            | ControllerBlockReason::SameTurnObservation
            | ControllerBlockReason::RepairInspectionRequired
            | ControllerBlockReason::NoEffect
            | ControllerBlockReason::SyntaxRegression
    );
    let unsupported_path_mutation =
        block.reason == ControllerBlockReason::UnsupportedMutation && block.check_name.is_none();
    if is_single_target_content_tool(&call.name)
        && (path_bearing || unsupported_path_mutation)
        && paths != targets
    {
        return Err(format!(
            "block paths {:?} do not exactly match ToolCall({}) target",
            block.paths, call.id
        ));
    }
    if matches!(
        call.name.as_str(),
        "copy_file" | "move_path" | "delete_path" | "make_dir"
    ) && block.reason == ControllerBlockReason::UnsupportedMutation
        && paths != targets
    {
        return Err(format!(
            "unsupported structural block paths {:?} do not match ToolCall({}) targets",
            block.paths, call.id
        ));
    }
    for path in &block.paths {
        if !targets.contains(path) {
            return Err(format!(
                "path {path:?} is not a target of ToolCall({})",
                call.id
            ));
        }
    }
    Ok(())
}

fn is_single_target_content_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "edit_file" | "multi_edit" | "apply_patch"
    )
}

fn statically_known_call_targets(call: &ToolCall) -> Result<Option<BTreeSet<String>>, String> {
    let keys: &[&str] = match call.name.as_str() {
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" | "delete_path" | "make_dir" => {
            &["path"]
        }
        "copy_file" | "move_path" => &["from", "to"],
        "run_check" => &[],
        _ => return Ok(None),
    };
    let mut targets = BTreeSet::new();
    for key in keys {
        targets.insert(normalized_call_path(required_string_arg(call, key)?)?);
    }
    Ok(Some(targets))
}

fn required_string_arg<'a>(call: &'a ToolCall, key: &str) -> Result<&'a str, String> {
    call.args
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("ToolCall({}) lacks string arg {key:?}", call.id))
}

fn optional_u64_arg(call: &ToolCall, key: &str) -> Result<Option<u64>, String> {
    let Some(value) = call.args.get(key) else {
        return Ok(None);
    };
    value
        .as_u64()
        .filter(|value| *value > 0)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "ToolCall({}) has invalid positive integer arg {key:?}",
                call.id
            )
        })
}

fn normalized_call_path(path: &str) -> Result<String, String> {
    let path = path.replace('\\', "/");
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        return Err(format!("ToolCall path {path:?} is not workspace-relative"));
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("ToolCall path {path:?} escapes the workspace"));
                }
            }
            part => parts.push(part),
        }
    }
    Ok(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

fn is_completion_control(name: &str) -> bool {
    matches!(name, TASK_COMPLETE | SUBMIT_PLAN)
}

fn is_unmeasured_read_only_or_control(name: &str) -> bool {
    matches!(name, "list_dir" | "fetch_reference" | REQUEST_USER_INPUT)
        || is_completion_control(name)
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

    fn evidence_base(validator: &mut TraceStructure, required_checks: &[&str]) {
        validator.observe(&policy(HarnessPolicy::Evidence)).unwrap();
        validator
            .observe(&Event::SessionPrompt {
                system: "system".to_string(),
                user: "task".to_string(),
                media: Vec::new(),
            })
            .unwrap();
        let state = ControllerState::new(
            HarnessPolicy::Evidence,
            required_checks.iter().map(|name| (*name).to_string()),
        )
        .unwrap()
        .checkpoint();
        validator
            .observe(&Event::ControllerCheckpoint { state })
            .unwrap();
    }

    fn begin_turn(validator: &mut TraceStructure, turn: u32, calls: Vec<ToolCall>) {
        validator.observe(&Event::TurnStart { turn }).unwrap();
        validator
            .observe(&Event::TurnEnd {
                turn,
                text: None,
                tool_call_count: calls.len() as u32,
                input_tokens: None,
                output_tokens: None,
                truncated: false,
            })
            .unwrap();
        validator
            .observe(&Event::ActionsProposed { turn, calls })
            .unwrap();
    }

    fn tool_call_event(call: &ToolCall) -> Event {
        Event::ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.args.clone(),
        }
    }

    fn full_file_observation(turn: u32, call_id: &str, path: &str, digest: char) -> Event {
        Event::ObservationRecorded {
            turn,
            call_id: call_id.to_string(),
            observation: ferric_trace::ObservationV1 {
                version: ferric_trace::CONTROLLER_RECORD_VERSION,
                detail: ferric_trace::ObservationDetailV1::File(ferric_trace::FileObservationV1 {
                    path: path.to_string(),
                    sha256: digest.to_string().repeat(64),
                    total_bytes: 4,
                    total_lines: 2,
                    requested_range: None,
                    returned_range: Some(ferric_trace::LineRangeV1 { start: 1, end: 2 }),
                    complete: true,
                    model_truncated: false,
                }),
            },
        }
    }

    fn commit(validator: &mut TraceStructure, turn: u32, dispatched: u32, errored: u32) {
        validator
            .observe(&Event::TurnCommitted {
                turn,
                dispatched,
                errored,
                stop_reason: None,
                snapshot_commit: None,
            })
            .unwrap();
    }

    fn establish_prior_read(validator: &mut TraceStructure) {
        let read = call("read-1", "read_file");
        begin_turn(validator, 0, vec![read.clone()]);
        validator.observe(&tool_call_event(&read)).unwrap();
        validator
            .observe(&full_file_observation(0, "read-1", "a.txt", 'a'))
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: "read-1".to_string(),
                name: "read_file".to_string(),
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap();
        commit(validator, 0, 1, 0);
    }

    fn record_passing_check(validator: &mut TraceStructure, turn: u32, name: &str) {
        let check = ToolCall {
            id: format!("check-{turn}"),
            name: "run_check".to_string(),
            args: json!({"name": name}),
        };
        begin_turn(validator, turn, vec![check.clone()]);
        validator.observe(&tool_call_event(&check)).unwrap();
        validator
            .observe(&Event::VerificationCheckRecorded {
                turn,
                call_id: check.id.clone(),
                check: ferric_trace::VerificationCheckV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    name: name.to_string(),
                    mutation_epoch: 0,
                    attempt: 1,
                    outcome: VerificationOutcome::Passed,
                    diagnostic_sha256: None,
                },
            })
            .unwrap();
        validator
            .observe(&Event::VerificationCheckPassed {
                turn,
                name: name.to_string(),
                mutation_epoch: 0,
            })
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: check.id,
                name: check.name,
                output: "passed".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap();
        commit(validator, turn, 1, 0);
    }

    fn modified_effect(turn: u32, call_id: &str, path: &str) -> Event {
        Event::WorkspaceEffectRecorded {
            turn,
            call_id: call_id.to_string(),
            tool: "write_file".to_string(),
            effect: ferric_trace::WorkspaceEffectV1 {
                version: ferric_trace::CONTROLLER_RECORD_VERSION,
                mutation_epoch: 1,
                effects: vec![ferric_trace::PathEffectV1 {
                    path: path.to_string(),
                    kind: ferric_trace::PathEffectKind::Modified,
                    before_sha256: Some("a".repeat(64)),
                    after_sha256: Some("b".repeat(64)),
                    after_bytes: Some(6),
                    after_lines: Some(3),
                }],
            },
        }
    }

    fn core_checkpoint(
        next_turn: u32,
        pending_input: Option<ferric_core::UserInputRequest>,
    ) -> RecoveryCheckpointV1 {
        RecoveryCheckpointV1 {
            version: ferric_trace::RECOVERY_CHECKPOINT_VERSION,
            messages: Vec::new(),
            next_turn,
            last_text: None,
            head_len: 0,
            committed_turn_starts: Vec::new(),
            guard_history: Vec::new(),
            nudged_for_no_action: false,
            truncated_once: false,
            last_input_tokens: None,
            pending_input,
            mutation_epoch: 0,
            passed_checks: BTreeMap::new(),
        }
    }

    fn resumed_evidence_base(
        validator: &mut TraceStructure,
        pause_reason: &str,
        pending_input: Option<ferric_core::UserInputRequest>,
    ) -> ControllerState {
        validator.observe(&policy(HarnessPolicy::Evidence)).unwrap();
        let core = core_checkpoint(3, pending_input);
        validator
            .observe(&Event::RecoveryCheckpoint { state: core })
            .unwrap();
        let pause = ControllerState::new(HarnessPolicy::Evidence, Vec::new())
            .unwrap()
            .checkpoint_for_pause(pause_reason)
            .unwrap();
        let resumed = ControllerState::resume_conservatively(&pause).unwrap();
        validator
            .observe(&Event::ControllerCheckpoint {
                state: resumed.checkpoint(),
            })
            .unwrap();
        resumed
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
    fn evidence_policy_accepts_a_causally_matched_observation() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &[]);
        let read = call("read-1", "read_file");
        begin_turn(&mut validator, 0, vec![read.clone()]);
        validator.observe(&tool_call_event(&read)).unwrap();
        validator
            .observe(&full_file_observation(0, "read-1", "a.txt", 'a'))
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: "read-1".to_string(),
                name: "read_file".to_string(),
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap();
        commit(&mut validator, 0, 1, 0);
        validator.finish().unwrap();
    }

    #[test]
    fn history_compaction_does_not_change_projected_controller_truth() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &[]);
        establish_prior_read(&mut validator);
        let before = validator.controller_checkpoint().unwrap();

        validator
            .observe(&Event::HistoryCompacted {
                through_turn: 0,
                dropped_turns: 1,
                summary: "earlier model history".to_string(),
            })
            .unwrap();

        assert_eq!(validator.controller_checkpoint(), Some(before));
    }

    #[test]
    fn evidence_policy_does_not_treat_git_read_as_an_unmeasured_safe_read() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &[]);
        let call = ToolCall {
            id: "git-1".to_string(),
            name: "git_read".to_string(),
            args: json!({"subcommand": "branch", "args": ["-D", "dev"]}),
        };
        begin_turn(&mut validator, 0, vec![call.clone()]);
        validator.observe(&tool_call_event(&call)).unwrap();
        let error = validator
            .observe(&Event::ToolResult {
                id: call.id,
                name: call.name,
                output: "unexpected mutation-capable git invocation".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap_err();
        assert!(error.contains("lacks a typed controller record"), "{error}");
    }

    #[test]
    fn evidence_planner_fails_closed_until_its_trace_protocol_exists() {
        let mut validator = TraceStructure::new();
        let error = validator
            .observe(&policy(HarnessPolicy::EvidencePlanner))
            .unwrap_err();
        assert!(error.contains("planner trace protocol"), "{error}");
    }

    #[test]
    fn observation_payload_must_match_path_and_literal_range_arguments() {
        let mut path_validator = TraceStructure::new();
        evidence_base(&mut path_validator, &[]);
        let read = call("read-1", "read_file");
        begin_turn(&mut path_validator, 0, vec![read.clone()]);
        path_validator.observe(&tool_call_event(&read)).unwrap();
        let error = path_validator
            .observe(&full_file_observation(0, "read-1", "b.txt", 'a'))
            .unwrap_err();
        assert!(error.contains("path does not match"), "{error}");

        let mut range_validator = TraceStructure::new();
        evidence_base(&mut range_validator, &[]);
        let ranged = ToolCall {
            id: "read-2".to_string(),
            name: "read_file".to_string(),
            args: json!({"path":"a.txt","start_line":2}),
        };
        begin_turn(&mut range_validator, 0, vec![ranged.clone()]);
        range_validator.observe(&tool_call_event(&ranged)).unwrap();
        let error = range_validator
            .observe(&full_file_observation(0, "read-2", "a.txt", 'a'))
            .unwrap_err();
        assert!(error.contains("requested range"), "{error}");
    }

    #[test]
    fn navigation_payload_must_match_root_literal_and_cap_arguments() {
        fn forged_error(field: &str) -> String {
            let mut validator = TraceStructure::new();
            evidence_base(&mut validator, &[]);
            let search = ToolCall {
                id: "search-1".to_string(),
                name: "search_files".to_string(),
                args: json!({"query":"needle","path":"src","max_results":7}),
            };
            begin_turn(&mut validator, 0, vec![search.clone()]);
            validator.observe(&tool_call_event(&search)).unwrap();
            let mut navigation = ferric_trace::NavigationObservationV1 {
                root: "src".to_string(),
                literal: "needle".to_string(),
                match_count: 0,
                max_results: 7,
                exhausted: true,
                result_sha256: "0".repeat(64),
            };
            match field {
                "root" => navigation.root = "tests".to_string(),
                "literal" => navigation.literal = "other".to_string(),
                "cap" => navigation.max_results = 8,
                _ => unreachable!(),
            }
            validator
                .observe(&Event::ObservationRecorded {
                    turn: 0,
                    call_id: "search-1".to_string(),
                    observation: ferric_trace::ObservationV1 {
                        version: ferric_trace::CONTROLLER_RECORD_VERSION,
                        detail: ferric_trace::ObservationDetailV1::Search(navigation),
                    },
                })
                .unwrap_err()
        }

        assert!(forged_error("root").contains("root does not match"));
        assert!(forged_error("literal").contains("literal query"));
        assert!(forged_error("cap").contains("cap does not match"));
    }

    #[test]
    fn same_turn_observation_cannot_authorize_a_later_call_in_the_batch() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &[]);
        let read = call("read-1", "read_file");
        let write = call("write-1", "write_file");
        begin_turn(&mut validator, 0, vec![read.clone(), write.clone()]);
        validator.observe(&tool_call_event(&read)).unwrap();
        validator
            .observe(&full_file_observation(0, "read-1", "a.txt", 'a'))
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: "read-1".to_string(),
                name: "read_file".to_string(),
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap();
        validator.observe(&tool_call_event(&write)).unwrap();
        let error = validator
            .observe(&modified_effect(0, "write-1", "a.txt"))
            .unwrap_err();
        assert!(error.contains("SameTurnObservation"), "{error}");
    }

    #[test]
    fn errored_result_may_follow_a_measured_effect_and_exact_legacy_mirror() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &[]);
        establish_prior_read(&mut validator);
        let write = call("write-1", "write_file");
        begin_turn(&mut validator, 1, vec![write.clone()]);
        validator.observe(&tool_call_event(&write)).unwrap();
        validator
            .observe(&modified_effect(1, "write-1", "a.txt"))
            .unwrap();
        validator
            .observe(&Event::WorkspaceMutation {
                turn: 1,
                tool: "write_file".to_string(),
                mutation_epoch: 1,
            })
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: "write-1".to_string(),
                name: "write_file".to_string(),
                output: "write failed after a partial effect".to_string(),
                is_error: true,
                duration_ms: 1,
            })
            .unwrap();
        commit(&mut validator, 1, 1, 1);
    }

    #[test]
    fn typed_effect_requires_compatibility_mirror_and_call_target() {
        let mut mirror_validator = TraceStructure::new();
        evidence_base(&mut mirror_validator, &[]);
        establish_prior_read(&mut mirror_validator);
        let write = call("write-1", "write_file");
        begin_turn(&mut mirror_validator, 1, vec![write.clone()]);
        mirror_validator.observe(&tool_call_event(&write)).unwrap();
        mirror_validator
            .observe(&modified_effect(1, "write-1", "a.txt"))
            .unwrap();
        let error = mirror_validator
            .observe(&Event::ToolResult {
                id: "write-1".to_string(),
                name: "write_file".to_string(),
                output: "ok".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap_err();
        assert!(
            error.contains("lacks its compatibility WorkspaceMutation"),
            "{error}"
        );

        let mut target_validator = TraceStructure::new();
        evidence_base(&mut target_validator, &[]);
        establish_prior_read(&mut target_validator);
        let write = call("write-1", "write_file");
        begin_turn(&mut target_validator, 1, vec![write.clone()]);
        target_validator.observe(&tool_call_event(&write)).unwrap();
        let error = target_validator
            .observe(&modified_effect(1, "write-1", "b.txt"))
            .unwrap_err();
        assert!(error.contains("do not exactly match"), "{error}");

        for extra in [false, true] {
            let mut validator = TraceStructure::new();
            evidence_base(&mut validator, &[]);
            establish_prior_read(&mut validator);
            let write = call("write-1", "write_file");
            begin_turn(&mut validator, 1, vec![write.clone()]);
            validator.observe(&tool_call_event(&write)).unwrap();
            let mut event = modified_effect(1, "write-1", "a.txt");
            let Event::WorkspaceEffectRecorded { effect, .. } = &mut event else {
                unreachable!()
            };
            if extra {
                effect.effects.push(ferric_trace::PathEffectV1 {
                    path: "b.txt".to_string(),
                    kind: ferric_trace::PathEffectKind::Created,
                    before_sha256: None,
                    after_sha256: Some("c".repeat(64)),
                    after_bytes: Some(2),
                    after_lines: Some(1),
                });
            } else {
                effect.effects.clear();
            }
            let error = validator.observe(&event).unwrap_err();
            assert!(error.contains("do not exactly match"), "{error}");
        }
    }

    #[test]
    fn unknown_tool_cannot_forge_a_typed_workspace_effect() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &[]);
        let call = ToolCall {
            id: "opaque-1".to_string(),
            name: "custom_mutator".to_string(),
            args: json!({"path": "a.txt"}),
        };
        begin_turn(&mut validator, 0, vec![call.clone()]);
        validator.observe(&tool_call_event(&call)).unwrap();
        let error = validator
            .observe(&Event::WorkspaceEffectRecorded {
                turn: 0,
                call_id: call.id,
                tool: call.name,
                effect: ferric_trace::WorkspaceEffectV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    mutation_epoch: 1,
                    effects: vec![ferric_trace::PathEffectV1 {
                        path: "a.txt".to_string(),
                        kind: ferric_trace::PathEffectKind::Created,
                        before_sha256: None,
                        after_sha256: Some("a".repeat(64)),
                        after_bytes: Some(1),
                        after_lines: Some(1),
                    }],
                },
            })
            .unwrap_err();
        assert!(error.contains("unsupported effect tool"), "{error}");
    }

    #[test]
    fn controller_block_target_and_stale_witness_are_verified() {
        let mut target_validator = TraceStructure::new();
        evidence_base(&mut target_validator, &[]);
        let write = call("write-1", "write_file");
        begin_turn(&mut target_validator, 0, vec![write.clone()]);
        target_validator.observe(&tool_call_event(&write)).unwrap();
        let error = target_validator
            .observe(&Event::ControllerBlocked {
                turn: 0,
                call_id: "write-1".to_string(),
                tool: "write_file".to_string(),
                block: ferric_trace::ControllerBlockV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    reason: ControllerBlockReason::BlindMutation,
                    mutation_epoch: 0,
                    paths: vec!["b.txt".to_string()],
                    check_name: None,
                    witness: None,
                },
            })
            .unwrap_err();
        assert!(error.contains("do not exactly match"), "{error}");

        let mut missing_validator = TraceStructure::new();
        evidence_base(&mut missing_validator, &[]);
        let write = call("write-1", "write_file");
        begin_turn(&mut missing_validator, 0, vec![write.clone()]);
        missing_validator.observe(&tool_call_event(&write)).unwrap();
        let error = missing_validator
            .observe(&Event::ControllerBlocked {
                turn: 0,
                call_id: "write-1".to_string(),
                tool: "write_file".to_string(),
                block: ferric_trace::ControllerBlockV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    reason: ControllerBlockReason::BlindMutation,
                    mutation_epoch: 0,
                    paths: Vec::new(),
                    check_name: None,
                    witness: None,
                },
            })
            .unwrap_err();
        assert!(error.contains("do not exactly match"), "{error}");

        let mut stale_validator = TraceStructure::new();
        evidence_base(&mut stale_validator, &[]);
        establish_prior_read(&mut stale_validator);
        let write = call("write-1", "write_file");
        begin_turn(&mut stale_validator, 1, vec![write.clone()]);
        stale_validator.observe(&tool_call_event(&write)).unwrap();
        let stale = Event::ControllerBlocked {
            turn: 1,
            call_id: "write-1".to_string(),
            tool: "write_file".to_string(),
            block: ferric_trace::ControllerBlockV1 {
                version: ferric_trace::CONTROLLER_RECORD_VERSION,
                reason: ControllerBlockReason::StaleObservation,
                mutation_epoch: 0,
                paths: vec!["a.txt".to_string()],
                check_name: None,
                witness: Some(ferric_trace::ControllerBlockWitnessV1::StaleObservation {
                    expected: ferric_trace::PreparedPathIdentityV1::File {
                        sha256: "a".repeat(64),
                        bytes: 4,
                    },
                    current: ferric_trace::PreparedPathIdentityV1::File {
                        sha256: "b".repeat(64),
                        bytes: 4,
                    },
                }),
            },
        };
        stale_validator.observe(&stale).unwrap();

        let mut identical_validator = TraceStructure::new();
        evidence_base(&mut identical_validator, &[]);
        establish_prior_read(&mut identical_validator);
        let write = call("write-1", "write_file");
        begin_turn(&mut identical_validator, 1, vec![write.clone()]);
        identical_validator
            .observe(&tool_call_event(&write))
            .unwrap();
        let mut identical = stale;
        let Event::ControllerBlocked { block, .. } = &mut identical else {
            unreachable!()
        };
        block.witness = Some(ferric_trace::ControllerBlockWitnessV1::StaleObservation {
            expected: ferric_trace::PreparedPathIdentityV1::File {
                sha256: "a".repeat(64),
                bytes: 4,
            },
            current: ferric_trace::PreparedPathIdentityV1::File {
                sha256: "a".repeat(64),
                bytes: 4,
            },
        });
        let error = identical_validator.observe(&identical).unwrap_err();
        assert!(error.contains("identical identities"), "{error}");
    }

    #[test]
    fn unsupported_mutation_blocks_must_name_exact_static_targets() {
        let cases = [
            ("write_file", json!({"path": "a.txt"})),
            ("delete_path", json!({"path": "a.txt"})),
            ("make_dir", json!({"path": "a"})),
            ("copy_file", json!({"from": "a.txt", "to": "b.txt"})),
            ("move_path", json!({"from": "a.txt", "to": "b.txt"})),
        ];
        for (tool, args) in cases {
            let mut validator = TraceStructure::new();
            evidence_base(&mut validator, &[]);
            let call = ToolCall {
                id: "call-1".to_string(),
                name: tool.to_string(),
                args,
            };
            begin_turn(&mut validator, 0, vec![call.clone()]);
            validator.observe(&tool_call_event(&call)).unwrap();
            let error = validator
                .observe(&Event::ControllerBlocked {
                    turn: 0,
                    call_id: call.id.clone(),
                    tool: tool.to_string(),
                    block: ferric_trace::ControllerBlockV1 {
                        version: ferric_trace::CONTROLLER_RECORD_VERSION,
                        reason: ControllerBlockReason::UnsupportedMutation,
                        mutation_epoch: 0,
                        paths: Vec::new(),
                        check_name: None,
                        witness: Some(
                            ferric_trace::ControllerBlockWitnessV1::UnsupportedMutation {
                                control_kind:
                                    ferric_trace::UnsupportedMutationKindV1::UnsupportedOperation,
                            },
                        ),
                    },
                })
                .unwrap_err();
            assert!(error.contains("do not"), "{tool}: {error}");
        }
    }

    #[test]
    fn unsupported_check_block_must_match_the_active_unconfigured_name() {
        fn unsupported_check(name: &str) -> Event {
            Event::ControllerBlocked {
                turn: 0,
                call_id: "check-1".to_string(),
                tool: "run_check".to_string(),
                block: ferric_trace::ControllerBlockV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    reason: ControllerBlockReason::UnsupportedMutation,
                    mutation_epoch: 0,
                    paths: Vec::new(),
                    check_name: Some(name.to_string()),
                    witness: Some(
                        ferric_trace::ControllerBlockWitnessV1::UnsupportedMutation {
                            control_kind:
                                ferric_trace::UnsupportedMutationKindV1::UnsupportedOperation,
                        },
                    ),
                },
            }
        }

        let check_call = ToolCall {
            id: "check-1".to_string(),
            name: "run_check".to_string(),
            args: json!({"name":"unknown"}),
        };
        let mut valid = TraceStructure::new();
        evidence_base(&mut valid, &["unit"]);
        begin_turn(&mut valid, 0, vec![check_call.clone()]);
        valid.observe(&tool_call_event(&check_call)).unwrap();
        valid.observe(&unsupported_check("unknown")).unwrap();
        valid
            .observe(&Event::ToolResult {
                id: "check-1".to_string(),
                name: "run_check".to_string(),
                output: "unknown check".to_string(),
                is_error: true,
                duration_ms: 1,
            })
            .unwrap();

        let mut forged = TraceStructure::new();
        evidence_base(&mut forged, &["unit"]);
        begin_turn(&mut forged, 0, vec![check_call.clone()]);
        forged.observe(&tool_call_event(&check_call)).unwrap();
        let error = forged.observe(&unsupported_check("other")).unwrap_err();
        assert!(error.contains("does not match ToolCall args"), "{error}");
    }

    #[test]
    fn no_effect_and_syntax_blocks_require_truthful_typed_witnesses() {
        fn prepared_file(digest: char) -> ferric_trace::PreparedPathIdentityV1 {
            ferric_trace::PreparedPathIdentityV1::File {
                sha256: digest.to_string().repeat(64),
                bytes: 4,
            }
        }
        fn block_event(witness: ferric_trace::ControllerBlockWitnessV1) -> Event {
            let reason = match &witness {
                ferric_trace::ControllerBlockWitnessV1::NoEffect { .. } => {
                    ControllerBlockReason::NoEffect
                }
                ferric_trace::ControllerBlockWitnessV1::SyntaxRegression { .. } => {
                    ControllerBlockReason::SyntaxRegression
                }
                _ => unreachable!(),
            };
            Event::ControllerBlocked {
                turn: 0,
                call_id: "write-1".to_string(),
                tool: "write_file".to_string(),
                block: ferric_trace::ControllerBlockV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    reason,
                    mutation_epoch: 0,
                    paths: vec!["a.txt".to_string()],
                    check_name: None,
                    witness: Some(witness),
                },
            }
        }
        fn prepared_validator() -> TraceStructure {
            let mut validator = TraceStructure::new();
            evidence_base(&mut validator, &[]);
            let write = call("write-1", "write_file");
            begin_turn(&mut validator, 0, vec![write.clone()]);
            validator.observe(&tool_call_event(&write)).unwrap();
            validator
        }

        let equal = ferric_trace::ControllerBlockWitnessV1::NoEffect {
            states: vec![ferric_trace::PreparedPathStateV1 {
                path: "a.txt".to_string(),
                before: prepared_file('a'),
                candidate: prepared_file('a'),
            }],
        };
        prepared_validator().observe(&block_event(equal)).unwrap();

        let changed = ferric_trace::ControllerBlockWitnessV1::NoEffect {
            states: vec![ferric_trace::PreparedPathStateV1 {
                path: "a.txt".to_string(),
                before: prepared_file('a'),
                candidate: prepared_file('b'),
            }],
        };
        let error = prepared_validator()
            .observe(&block_event(changed))
            .unwrap_err();
        assert!(error.contains("materially different"), "{error}");

        let absent_to_invalid = ferric_trace::ControllerBlockWitnessV1::SyntaxRegression {
            before: ferric_trace::SyntaxStateV1::Absent,
            candidate: ferric_trace::SyntaxStateV1::Invalid,
            diagnostic_sha256: "d".repeat(64),
        };
        prepared_validator()
            .observe(&block_event(absent_to_invalid))
            .unwrap();

        let unchecked = ferric_trace::ControllerBlockWitnessV1::SyntaxRegression {
            before: ferric_trace::SyntaxStateV1::Unchecked,
            candidate: ferric_trace::SyntaxStateV1::Invalid,
            diagnostic_sha256: "d".repeat(64),
        };
        let error = prepared_validator()
            .observe(&block_event(unchecked))
            .unwrap_err();
        assert!(error.contains("valid-to-invalid"), "{error}");
    }

    #[test]
    fn check_outcomes_attempts_and_same_epoch_uniqueness_are_causal() {
        let check_call = |id: &str| ToolCall {
            id: id.to_string(),
            name: "run_check".to_string(),
            args: json!({"name":"unit"}),
        };
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &["unit"]);
        let first = check_call("check-1");
        begin_turn(&mut validator, 0, vec![first.clone()]);
        validator.observe(&tool_call_event(&first)).unwrap();
        validator
            .observe(&Event::VerificationCheckRecorded {
                turn: 0,
                call_id: "check-1".to_string(),
                check: ferric_trace::VerificationCheckV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    name: "unit".to_string(),
                    mutation_epoch: 0,
                    attempt: 1,
                    outcome: VerificationOutcome::Failed,
                    diagnostic_sha256: Some("f".repeat(64)),
                },
            })
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: "check-1".to_string(),
                name: "run_check".to_string(),
                output: "failed".to_string(),
                is_error: true,
                duration_ms: 1,
            })
            .unwrap();
        commit(&mut validator, 0, 1, 1);

        let repeated = check_call("check-2");
        begin_turn(&mut validator, 1, vec![repeated.clone()]);
        validator.observe(&tool_call_event(&repeated)).unwrap();
        validator
            .observe(&Event::ControllerBlocked {
                turn: 1,
                call_id: "check-2".to_string(),
                tool: "run_check".to_string(),
                block: ferric_trace::ControllerBlockV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    reason: ControllerBlockReason::RepeatedCheck,
                    mutation_epoch: 0,
                    paths: Vec::new(),
                    check_name: Some("unit".to_string()),
                    witness: None,
                },
            })
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: "check-2".to_string(),
                name: "run_check".to_string(),
                output: "already ran".to_string(),
                is_error: true,
                duration_ms: 1,
            })
            .unwrap();

        let mut pass_without_mirror = TraceStructure::new();
        evidence_base(&mut pass_without_mirror, &["unit"]);
        let passing = check_call("check-1");
        begin_turn(&mut pass_without_mirror, 0, vec![passing.clone()]);
        pass_without_mirror
            .observe(&tool_call_event(&passing))
            .unwrap();
        pass_without_mirror
            .observe(&Event::VerificationCheckRecorded {
                turn: 0,
                call_id: "check-1".to_string(),
                check: ferric_trace::VerificationCheckV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    name: "unit".to_string(),
                    mutation_epoch: 0,
                    attempt: 1,
                    outcome: VerificationOutcome::Passed,
                    diagnostic_sha256: None,
                },
            })
            .unwrap();
        let error = pass_without_mirror
            .observe(&Event::ToolResult {
                id: "check-1".to_string(),
                name: "run_check".to_string(),
                output: "passed".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap_err();
        assert!(error.contains("lacks its compatibility"), "{error}");
    }

    #[test]
    fn global_repair_barrier_block_is_bound_to_the_new_attempted_target() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &["unit"]);
        let check = ToolCall {
            id: "check-1".to_string(),
            name: "run_check".to_string(),
            args: json!({"name": "unit"}),
        };
        begin_turn(&mut validator, 0, vec![check.clone()]);
        validator.observe(&tool_call_event(&check)).unwrap();
        validator
            .observe(&Event::VerificationCheckRecorded {
                turn: 0,
                call_id: check.id.clone(),
                check: ferric_trace::VerificationCheckV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    name: "unit".to_string(),
                    mutation_epoch: 0,
                    attempt: 1,
                    outcome: VerificationOutcome::Failed,
                    diagnostic_sha256: Some("f".repeat(64)),
                },
            })
            .unwrap();
        validator
            .observe(&Event::ToolResult {
                id: check.id,
                name: check.name,
                output: "failed".to_string(),
                is_error: true,
                duration_ms: 1,
            })
            .unwrap();
        commit(&mut validator, 0, 1, 1);

        let write = ToolCall {
            id: "write-1".to_string(),
            name: "write_file".to_string(),
            args: json!({"path": "new.txt", "content": "new"}),
        };
        begin_turn(&mut validator, 1, vec![write.clone()]);
        validator.observe(&tool_call_event(&write)).unwrap();
        validator
            .observe(&Event::ControllerBlocked {
                turn: 1,
                call_id: write.id,
                tool: write.name,
                block: ferric_trace::ControllerBlockV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    reason: ControllerBlockReason::RepairInspectionRequired,
                    mutation_epoch: 0,
                    paths: vec!["new.txt".to_string()],
                    check_name: None,
                    witness: None,
                },
            })
            .unwrap();
    }

    #[test]
    fn evidence_completion_gate_cannot_erase_required_checks() {
        let mut validator = TraceStructure::new();
        evidence_base(&mut validator, &["unit"]);
        begin_turn(&mut validator, 0, Vec::new());
        let error = validator
            .observe(&Event::CompletionGate {
                mutation_epoch: 0,
                required_checks: Vec::new(),
                fresh_checks: Vec::new(),
                decision: "passed".to_string(),
            })
            .unwrap_err();
        assert!(error.contains("controller configuration"), "{error}");
    }

    #[test]
    fn successful_evidence_completion_requires_a_current_active_turn_gate() {
        let completion = ToolCall {
            id: "complete-1".to_string(),
            name: TASK_COMPLETE.to_string(),
            args: json!({"summary": "done"}),
        };

        let mut omitted = TraceStructure::new();
        evidence_base(&mut omitted, &["unit"]);
        record_passing_check(&mut omitted, 0, "unit");
        begin_turn(&mut omitted, 1, vec![completion.clone()]);
        omitted.observe(&tool_call_event(&completion)).unwrap();
        let error = omitted
            .observe(&Event::TurnCommitted {
                turn: 1,
                dispatched: 0,
                errored: 0,
                stop_reason: Some("task_complete".to_string()),
                snapshot_commit: None,
            })
            .unwrap_err();
        assert!(error.contains("current passed CompletionGate"), "{error}");

        let mut final_text = TraceStructure::new();
        evidence_base(&mut final_text, &["unit"]);
        record_passing_check(&mut final_text, 0, "unit");
        begin_turn(&mut final_text, 1, Vec::new());
        let error = final_text
            .observe(&Event::TurnCommitted {
                turn: 1,
                dispatched: 0,
                errored: 0,
                stop_reason: Some("final_text".to_string()),
                snapshot_commit: None,
            })
            .unwrap_err();
        assert!(error.contains("current passed CompletionGate"), "{error}");

        let mut valid = TraceStructure::new();
        evidence_base(&mut valid, &["unit"]);
        record_passing_check(&mut valid, 0, "unit");
        begin_turn(&mut valid, 1, vec![completion.clone()]);
        valid.observe(&tool_call_event(&completion)).unwrap();
        valid
            .observe(&Event::CompletionGate {
                mutation_epoch: 0,
                required_checks: vec!["unit".to_string()],
                fresh_checks: vec!["unit".to_string()],
                decision: "passed".to_string(),
            })
            .unwrap();
        valid
            .observe(&Event::TurnCommitted {
                turn: 1,
                dispatched: 0,
                errored: 0,
                stop_reason: Some("task_complete".to_string()),
                snapshot_commit: None,
            })
            .unwrap();
    }

    #[test]
    fn evidence_completion_gate_is_unique_and_cannot_precede_a_later_effect() {
        let completion = ToolCall {
            id: "complete-1".to_string(),
            name: TASK_COMPLETE.to_string(),
            args: json!({"summary": "done"}),
        };
        let gate = Event::CompletionGate {
            mutation_epoch: 0,
            required_checks: vec!["unit".to_string()],
            fresh_checks: vec!["unit".to_string()],
            decision: "passed".to_string(),
        };

        let mut duplicate = TraceStructure::new();
        evidence_base(&mut duplicate, &["unit"]);
        record_passing_check(&mut duplicate, 0, "unit");
        begin_turn(&mut duplicate, 1, vec![completion.clone()]);
        duplicate.observe(&tool_call_event(&completion)).unwrap();
        duplicate.observe(&gate).unwrap();
        let error = duplicate.observe(&gate).unwrap_err();
        assert!(error.contains("more than one CompletionGate"), "{error}");

        let write = ToolCall {
            id: "write-1".to_string(),
            name: "write_file".to_string(),
            args: json!({"path": "new.txt", "content": "new"}),
        };
        let mut stale = TraceStructure::new();
        evidence_base(&mut stale, &["unit"]);
        record_passing_check(&mut stale, 0, "unit");
        begin_turn(&mut stale, 1, vec![completion.clone(), write.clone()]);
        stale.observe(&tool_call_event(&completion)).unwrap();
        stale.observe(&gate).unwrap();
        stale.observe(&tool_call_event(&write)).unwrap();
        stale
            .observe(&Event::WorkspaceEffectRecorded {
                turn: 1,
                call_id: write.id.clone(),
                tool: write.name.clone(),
                effect: ferric_trace::WorkspaceEffectV1 {
                    version: ferric_trace::CONTROLLER_RECORD_VERSION,
                    mutation_epoch: 1,
                    effects: vec![ferric_trace::PathEffectV1 {
                        path: "new.txt".to_string(),
                        kind: ferric_trace::PathEffectKind::Created,
                        before_sha256: None,
                        after_sha256: Some("b".repeat(64)),
                        after_bytes: Some(3),
                        after_lines: Some(1),
                    }],
                },
            })
            .unwrap();
        stale
            .observe(&Event::WorkspaceMutation {
                turn: 1,
                tool: write.name.clone(),
                mutation_epoch: 1,
            })
            .unwrap();
        stale
            .observe(&Event::ToolResult {
                id: write.id,
                name: write.name,
                output: "wrote new.txt".to_string(),
                is_error: false,
                duration_ms: 1,
            })
            .unwrap();
        let error = stale
            .observe(&Event::TurnCommitted {
                turn: 1,
                dispatched: 1,
                errored: 0,
                stop_reason: Some("task_complete".to_string()),
                snapshot_commit: None,
            })
            .unwrap_err();
        assert!(error.contains("current passed CompletionGate"), "{error}");
    }

    #[test]
    fn resumed_controller_checkpoint_must_match_core_coordinates() {
        let mut validator = TraceStructure::new();
        validator.observe(&policy(HarnessPolicy::Evidence)).unwrap();
        validator
            .observe(&Event::RecoveryCheckpoint {
                state: core_checkpoint(3, None),
            })
            .unwrap();
        let mut forged = ControllerState::new(HarnessPolicy::Evidence, Vec::new())
            .unwrap()
            .checkpoint_for_pause("max_turns")
            .unwrap();
        forged.mutation_epoch = 1;
        let error = validator
            .observe(&Event::ControllerCheckpoint { state: forged })
            .unwrap_err();
        assert!(error.contains("core recovery coordinates"), "{error}");
    }

    #[test]
    fn pending_clarification_requires_prompt_and_both_answer_anchors() {
        let pending = ferric_core::UserInputRequest {
            question: "Which file?".to_string(),
            context: "Two candidates".to_string(),
            options: vec!["a.txt".to_string(), "b.txt".to_string()],
        };

        let mut missing_prompt = TraceStructure::new();
        resumed_evidence_base(&mut missing_prompt, "needs_input", Some(pending.clone()));
        let error = missing_prompt
            .observe(&Event::TurnStart { turn: 3 })
            .unwrap_err();
        assert!(error.contains("pending clarification"), "{error}");

        let mut missing_core_anchor = TraceStructure::new();
        resumed_evidence_base(
            &mut missing_core_anchor,
            "needs_input",
            Some(pending.clone()),
        );
        missing_core_anchor
            .observe(&Event::ResumePrompt {
                user: "a.txt".to_string(),
                media: Vec::new(),
            })
            .unwrap();
        let error = missing_core_anchor
            .observe(&Event::TurnStart { turn: 3 })
            .unwrap_err();
        assert!(error.contains("answer anchor"), "{error}");

        let mut missing_controller_anchor = TraceStructure::new();
        resumed_evidence_base(&mut missing_controller_anchor, "needs_input", Some(pending));
        missing_controller_anchor
            .observe(&Event::ResumePrompt {
                user: "a.txt".to_string(),
                media: Vec::new(),
            })
            .unwrap();
        missing_controller_anchor
            .observe(&Event::RecoveryCheckpoint {
                state: core_checkpoint(3, None),
            })
            .unwrap();
        let error = missing_controller_anchor
            .observe(&Event::TurnStart { turn: 3 })
            .unwrap_err();
        assert!(error.contains("immediately follow"), "{error}");
    }

    #[test]
    fn recovery_packet_order_handles_no_amendment_generic_and_clarification_resumes() {
        // No amendment: the packet may follow the paired resume base directly.
        let mut direct = TraceStructure::new();
        let direct_state = resumed_evidence_base(&mut direct, "max_turns", None);
        let packet = direct_state.recovery_packet("max_turns").unwrap();
        let message = ControllerState::render_recovery_packet(&packet).unwrap();
        direct
            .observe(&Event::RecoveryPacketInjected {
                packet: packet.clone(),
                message: message.clone(),
            })
            .unwrap();
        direct.observe(&Event::TurnStart { turn: 3 }).unwrap();

        // A later amendment cannot appear after the packet.
        let mut forged = TraceStructure::new();
        let forged_state = resumed_evidence_base(&mut forged, "max_turns", None);
        let forged_packet = forged_state.recovery_packet("max_turns").unwrap();
        forged
            .observe(&Event::RecoveryPacketInjected {
                message: ControllerState::render_recovery_packet(&forged_packet).unwrap(),
                packet: forged_packet,
            })
            .unwrap();
        let error = forged
            .observe(&Event::ResumePrompt {
                user: "also update docs".to_string(),
                media: Vec::new(),
            })
            .unwrap_err();
        assert!(error.contains("after RecoveryPacketInjected"), "{error}");

        // Generic amendment: both core/controller answer anchors precede the
        // packet, which then precedes TurnStart.
        let mut generic = TraceStructure::new();
        let generic_state = resumed_evidence_base(&mut generic, "max_turns", None);
        generic
            .observe(&Event::ResumePrompt {
                user: "also update docs".to_string(),
                media: Vec::new(),
            })
            .unwrap();
        generic
            .observe(&Event::RecoveryCheckpoint {
                state: core_checkpoint(3, None),
            })
            .unwrap();
        generic
            .observe(&Event::ControllerCheckpoint {
                state: generic_state.checkpoint(),
            })
            .unwrap();
        let packet = generic_state.recovery_packet("max_turns").unwrap();
        generic
            .observe(&Event::RecoveryPacketInjected {
                message: ControllerState::render_recovery_packet(&packet).unwrap(),
                packet,
            })
            .unwrap();
        generic.observe(&Event::TurnStart { turn: 3 }).unwrap();

        // Clarification answer uses paired anchors but deliberately omits the
        // generic needs_input recovery prose.
        let pending = ferric_core::UserInputRequest {
            question: "Which file?".to_string(),
            context: "Two candidates".to_string(),
            options: vec!["a.txt".to_string(), "b.txt".to_string()],
        };
        let mut clarification = TraceStructure::new();
        let clarification_state =
            resumed_evidence_base(&mut clarification, "needs_input", Some(pending));
        clarification
            .observe(&Event::ResumePrompt {
                user: "a.txt".to_string(),
                media: Vec::new(),
            })
            .unwrap();
        clarification
            .observe(&Event::RecoveryCheckpoint {
                state: core_checkpoint(3, None),
            })
            .unwrap();
        clarification
            .observe(&Event::ControllerCheckpoint {
                state: clarification_state.checkpoint(),
            })
            .unwrap();
        clarification
            .observe(&Event::TurnStart { turn: 3 })
            .unwrap();
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
