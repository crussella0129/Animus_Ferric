use ferric_core::{ActionProtocol, Message, Role, ToolCall, UserInputRequest};
use ferric_trace::{
    Event, GuardTurn, RECOVERY_CHECKPOINT_VERSION, RecoveryCheckpointV1, TurnBoundary,
    VerificationOutcome,
};

// Formatting functions extracted from run.rs
pub(crate) fn no_action_nudge(protocol: ActionProtocol) -> &'static str {
    match protocol {
        ActionProtocol::NativeTools => "Respond with a tool call, or your final answer as text.",
        ActionProtocol::ConstrainedJson | ActionProtocol::Plan => {
            "Respond with a single JSON action: {\"tool\": \"tool_name\", \"args\": { ... }}"
        }
        ActionProtocol::TextXml => {
            "Respond with an XML tool call: <tool_call><name>tool_name</name><args>{\"arg\": \"value\"}</args></tool_call>"
        }
    }
}

pub(crate) fn truncation_retry_message() -> Message {
    Message::user("Your last action was cut off by the token limit. Re-issue it more concisely.")
}

pub(crate) fn repetition_warn_message(repeated: &[&str]) -> Message {
    Message::user(format!(
        "You already called {} and have the result — do not call it again. \
         If the task is finished, call task_complete now with a one-sentence summary.",
        repeated.join(", ")
    ))
}

pub(crate) fn no_progress_warn_message(repeated: &[&str]) -> Message {
    Message::user(format!(
        "You have called {} repeatedly without finishing. If the task is \
         complete, call task_complete now; otherwise use a different tool or \
         arguments that move toward the goal.",
        repeated.join(", ")
    ))
}

pub(crate) fn oscillation_warn_message() -> Message {
    Message::user(
        "You are cycling between the same few tool calls without making          progress. Their results will not change if you repeat them. Use what          you already have to take a DIFFERENT next step, or call task_complete          if the task is done.",
    )
}

pub(crate) fn failure_warn_message() -> Message {
    Message::user(
        "Your last tool call(s) failed. Read the error message and try a \
         different approach, or call task_complete if you cannot proceed.",
    )
}

/// Build the model-facing message for one tool result.
///
/// `output` arrives from the trace, which stores the **full** output for
/// durability (ADR-002). The context window gets the truncated view — applying
/// it here, at the single point where messages are assembled, is what keeps run
/// and replay identical.
pub(crate) fn result_message(
    protocol: ActionProtocol,
    call_id: &str,
    name: &str,
    output: &str,
    truncation_limit: usize,
) -> Message {
    let output = &ferric_tools::truncate_for_model(output, truncation_limit);
    match protocol {
        ActionProtocol::NativeTools => Message::tool_result(call_id, output),
        ActionProtocol::ConstrainedJson | ActionProtocol::TextXml | ActionProtocol::Plan => {
            Message::user(format!("[tool_result for {name}] {output}"))
        }
    }
}

/// One turn's data, buffered until a later `TurnStart` (or EOF) decides
/// whether it's confirmed complete or dangling.
#[derive(Default, Debug, Clone)]
pub struct PendingTurn {
    pub turn: u32,
    pub turn_end: Option<(Option<String>, bool)>,
    /// The complete decoded batch. `None` identifies legacy traces written
    /// before the durable proposal/commit protocol.
    pub actions_proposed: Option<Vec<ToolCall>>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<(String, String, String, bool)>,
    pub controller_blocks: u32,
    pub repetition_warned: bool,
    pub no_progress_warned: bool,
    pub failure_warned: bool,
    pub oscillation_warned: bool,
    pub completion_gate_feedback: Option<String>,
}

impl PendingTurn {
    /// Turn this buffer into the messages `run()` would have pushed for it,
    /// or `None` if it never received a `TurnEnd` at all (defensively treated
    /// the same as a dangling turn).
    fn finalize(self, protocol: ActionProtocol, truncation_limit: usize) -> Option<Vec<Message>> {
        let (text, truncated) = self.turn_end?;

        // Truncation handling is ConstrainedJson-only (run.rs mirrors this
        // exact gate) — other protocols ignore the flag and proceed normally.
        if truncated
            && (protocol == ActionProtocol::ConstrainedJson || protocol == ActionProtocol::Plan)
        {
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
            ActionProtocol::ConstrainedJson | ActionProtocol::TextXml | ActionProtocol::Plan => {
                Message::assistant(text.unwrap_or_default())
            }
        };

        if self.tool_calls.is_empty() {
            // No traced ToolCall events this turn: run() would have hit the
            // no-action nudge and `continue`d — no guards, no dispatch.
            let feedback = self
                .completion_gate_feedback
                .unwrap_or_else(|| no_action_nudge(protocol).to_string());
            return Some(vec![assistant, Message::user(feedback)]);
        }

        let mut out = vec![assistant];
        let names: Vec<&str> = self.tool_calls.iter().map(|c| c.name.as_str()).collect();
        if self.repetition_warned {
            out.push(repetition_warn_message(&names));
        }
        if self.no_progress_warned {
            out.push(no_progress_warn_message(&names));
        }
        if self.oscillation_warned {
            out.push(oscillation_warn_message());
        }
        for (id, name, output, _) in &self.tool_results {
            out.push(result_message(protocol, id, name, output, truncation_limit));
        }
        if self.failure_warned {
            out.push(failure_warn_message());
        }
        Some(out)
    }
}

/// A pure, event-sourced state machine that reconstructs the agent's context window
/// (`messages`) by observing `ferric_trace::Event`s. This eliminates the dual
/// maintenance of message-formatting logic between `run.rs` and `replay.rs`.
pub struct TraceProjector {
    pub protocol: Option<ActionProtocol>,
    pub messages: Vec<Message>,
    pub turns: u32,
    pub last_text: Option<String>,
    pub pending: Option<PendingTurn>,
    pub head_len: usize,
    pub committed_turn_starts: Vec<(u32, usize)>,
    /// Absolute next turn id across a resume chain.
    pub next_turn: u32,
    /// Action/result history needed to reconstruct the stateful guards.
    pub guard_history: Vec<GuardTurn>,
    pub nudged_for_no_action: bool,
    pub truncated_once: bool,
    pub last_input_tokens: Option<u32>,
    pub pending_input: Option<UserInputRequest>,
    pub mutation_epoch: u64,
    pub passed_checks: std::collections::BTreeMap<String, u64>,
    /// Model-facing cap on a single tool result (ADR-002). Set from the
    /// `PolicySelected` event and nowhere else, which is what makes run and
    /// replay agree: the projector must also work in replay, where there is no
    /// registry — only the trace — so the trace is the single source for both
    /// (ADR-093). Until that event arrives it holds the default, which is what
    /// pre-ADR-093 traces recorded implicitly.
    pub truncation_limit: usize,
}

impl TraceProjector {
    pub fn new() -> Self {
        Self {
            protocol: None,
            messages: Vec::new(),
            turns: 0,
            last_text: None,
            pending: None,
            head_len: 0,
            committed_turn_starts: Vec::new(),
            next_turn: 0,
            guard_history: Vec::new(),
            nudged_for_no_action: false,
            truncated_once: false,
            last_input_tokens: None,
            pending_input: None,
            mutation_epoch: 0,
            passed_checks: std::collections::BTreeMap::new(),
            truncation_limit: ferric_tools::DEFAULT_TRUNCATION_LIMIT,
        }
    }

    /// Feeds an event into the projector, updating the context window.
    pub fn step(&mut self, event: &Event) {
        match event {
            Event::PolicySelected {
                protocol,
                truncation_limit,
                ..
            } => {
                self.protocol = Some(*protocol);
                self.truncation_limit = *truncation_limit;
            }
            Event::SessionPrompt {
                system,
                user,
                media,
            } => {
                self.messages.push(Message::system(system.clone()));
                self.messages
                    .push(Message::user_with_media(user.clone(), media.clone()));
                self.head_len = self.messages.len();
            }
            Event::RecoveryCheckpoint { state } if state.version == RECOVERY_CHECKPOINT_VERSION => {
                self.messages = state.messages.clone();
                self.turns = state.next_turn;
                self.next_turn = state.next_turn;
                self.last_text = state.last_text.clone();
                self.head_len = state.head_len;
                self.committed_turn_starts = state
                    .committed_turn_starts
                    .iter()
                    .map(|boundary| (boundary.turn, boundary.message_index))
                    .collect();
                self.guard_history = state.guard_history.clone();
                self.nudged_for_no_action = state.nudged_for_no_action;
                self.truncated_once = state.truncated_once;
                self.last_input_tokens = state.last_input_tokens;
                self.pending_input = state.pending_input.clone();
                self.mutation_epoch = state.mutation_epoch;
                self.passed_checks = state.passed_checks.clone();
                self.pending = None;
            }
            Event::ResumePrompt { user, media } => {
                self.messages
                    .push(Message::user_with_media(user.clone(), media.clone()));
                // The answer/amendment and consumption of the pending request
                // are one durable transition. If the process crashes before
                // the following checkpoint, replay must not ask for the same
                // answer a second time.
                self.pending_input = None;
            }
            Event::RecoveryPacketInjected { message, .. } => {
                // The literal trace payload, not a newly rendered equivalent,
                // is the durable model-history source of truth.
                self.messages.push(Message::user(message.clone()));
            }
            Event::ControllerCheckpoint { .. } => {
                // Controller truth is projected by TraceStructure and remains
                // deliberately separate from model-visible message history.
            }
            Event::TurnStart { turn } => {
                // Pre-recovery traces used the next turn start as their commit
                // barrier. Modern turns carry ActionsProposed and must instead
                // wait for an explicit TurnCommitted event.
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.actions_proposed.is_none())
                {
                    self.commit_pending();
                }
                self.pending = Some(PendingTurn {
                    turn: *turn,
                    ..Default::default()
                });
            }
            Event::TurnEnd {
                text,
                input_tokens,
                truncated,
                ..
            } => {
                if let Some(p) = &mut self.pending {
                    p.turn_end = Some((text.clone(), *truncated));
                }
                self.last_input_tokens = *input_tokens;
            }
            Event::ActionsProposed { turn, calls } => {
                if let Some(p) = &mut self.pending
                    && p.turn == *turn
                {
                    p.actions_proposed = Some(calls.clone());
                }
            }
            Event::ToolCall { id, name, args } => {
                if let Some(p) = &mut self.pending {
                    p.tool_calls.push(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        args: args.clone(),
                    });
                }
            }
            Event::ToolResult {
                id,
                name,
                output,
                is_error,
                ..
            } => {
                if let Some(p) = &mut self.pending {
                    p.tool_results
                        .push((id.clone(), name.clone(), output.clone(), *is_error));
                }
            }
            Event::ControllerBlocked { turn, .. } => {
                if let Some(p) = &mut self.pending
                    && p.turn == *turn
                {
                    p.controller_blocks = p.controller_blocks.saturating_add(1);
                }
            }
            Event::WorkspaceMutation { mutation_epoch, .. } => {
                self.mutation_epoch = self.mutation_epoch.max(*mutation_epoch);
            }
            Event::VerificationCheckPassed {
                name,
                mutation_epoch,
                ..
            } => {
                self.passed_checks.insert(name.clone(), *mutation_epoch);
            }
            Event::VerificationCheckRecorded { check, .. }
                if check.outcome == VerificationOutcome::Failed =>
            {
                // A failed typed record is authoritative even though the
                // compatibility vocabulary has no corresponding failure event.
                self.passed_checks.remove(&check.name);
            }
            Event::TurnCommitted {
                turn,
                dispatched,
                errored,
                stop_reason,
                ..
            } => {
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.turn == *turn)
                {
                    self.commit_pending_with(*dispatched, *errored, stop_reason.as_deref());
                }
            }
            Event::CompletionGate {
                mutation_epoch,
                required_checks,
                fresh_checks,
                decision,
            } if decision == "blocked" => {
                if let Some(pending) = &mut self.pending {
                    pending.completion_gate_feedback = Some(completion_gate_message(
                        *mutation_epoch,
                        required_checks,
                        fresh_checks,
                    ));
                }
            }
            Event::RepetitionGuard { action } if action == "warned" => {
                if let Some(p) = &mut self.pending {
                    p.repetition_warned = true;
                }
            }
            Event::NoProgressGuard { action } if action == "warned" => {
                if let Some(p) = &mut self.pending {
                    p.no_progress_warned = true;
                }
            }
            Event::FailureGuard { action } if action == "warned" => {
                if let Some(p) = &mut self.pending {
                    p.failure_warned = true;
                }
            }
            Event::OscillationGuard { action } if action == "warned" => {
                if let Some(p) = &mut self.pending {
                    p.oscillation_warned = true;
                }
            }
            Event::HistoryCompacted {
                through_turn,
                summary,
                ..
            } => {
                let split_at = self
                    .committed_turn_starts
                    .partition_point(|&(t, _)| t <= *through_turn);
                if split_at > 0 {
                    let preserve_from_idx = self
                        .committed_turn_starts
                        .get(split_at)
                        .map(|&(_, idx)| idx)
                        .unwrap_or(self.messages.len());
                    let Some(new_base) = self.head_len.checked_add(1) else {
                        return;
                    };
                    if self.head_len > self.messages.len()
                        || preserve_from_idx < self.head_len
                        || preserve_from_idx > self.messages.len()
                    {
                        return;
                    }
                    let Some(remapped_boundaries) = self.committed_turn_starts[split_at..]
                        .iter()
                        .map(|&(turn, index)| {
                            index
                                .checked_sub(preserve_from_idx)
                                .and_then(|offset| new_base.checked_add(offset))
                                .map(|index| (turn, index))
                        })
                        .collect::<Option<Vec<_>>>()
                    else {
                        return;
                    };
                    let preserved_tail: Vec<Message> = self.messages[preserve_from_idx..].to_vec();
                    self.messages.truncate(self.head_len);
                    self.messages
                        .push(Message::user(format!("[compacted history] {summary}")));
                    self.messages.extend(preserved_tail);
                    self.committed_turn_starts = remapped_boundaries;
                }
            }
            _ => {}
        }
    }

    /// Commits the currently open pending turn (a later TurnStart confirms it
    /// finished dispatching) and appends its messages to the context window.
    pub fn commit_pending(&mut self) {
        let (dispatched, errored) = self
            .pending
            .as_ref()
            .map(|pending| {
                let dispatched = pending.tool_results.len() as u32;
                let errored = pending
                    .tool_results
                    .iter()
                    .filter(|(_, _, _, is_error)| *is_error)
                    .count() as u32;
                (dispatched, errored)
            })
            .unwrap_or_default();
        self.commit_pending_with(dispatched, errored, None);
    }

    /// Commit a turn at its durable barrier and retain enough control state to
    /// reconstruct the guards after a process restart.
    pub fn commit_pending_with(
        &mut self,
        dispatched: u32,
        errored: u32,
        stop_reason: Option<&str>,
    ) {
        if let (Some(p), Some(proto)) = (self.pending.take(), self.protocol) {
            let turn_num = p.turn;
            let proposed = p.actions_proposed.clone().unwrap_or_default();
            let truncated = p.turn_end.as_ref().is_some_and(|(_, value)| *value);
            let completion_was_blocked = p.completion_gate_feedback.is_some();
            let controller_blocks = p.controller_blocks;
            if stop_reason == Some("needs_input") && self.pending_input.is_none() {
                self.pending_input = proposed
                    .iter()
                    .find(|call| crate::terminator::is_request_user_input(&call.name))
                    .and_then(|call| crate::terminator::request_of(&call.args).ok());
            }
            if let Some(msgs) = p.finalize(proto, self.truncation_limit) {
                if proto == ActionProtocol::NativeTools
                    && let Some(assistant) = msgs.first()
                    && let Some(t) = &assistant.text
                    && !t.is_empty()
                {
                    self.last_text = Some(t.clone());
                }
                self.committed_turn_starts
                    .push((turn_num, self.messages.len()));
                self.messages.extend(msgs);
                self.turns += 1;
                self.next_turn = self.next_turn.max(turn_num.saturating_add(1));
                if truncated {
                    self.truncated_once = true;
                }
                if proposed.is_empty() && stop_reason.is_none() && !completion_was_blocked {
                    self.nudged_for_no_action = true;
                }
                if !proposed.is_empty() && stop_reason != Some("needs_input") {
                    self.guard_history.push(GuardTurn {
                        turn: turn_num,
                        calls: proposed,
                        dispatched,
                        errored,
                        controller_blocks,
                        controller_blocks_was_present: true,
                    });
                }
            }
        }
    }

    /// Capture a self-contained inherited-state anchor for a new trace.
    pub fn checkpoint(&self) -> RecoveryCheckpointV1 {
        RecoveryCheckpointV1 {
            version: RECOVERY_CHECKPOINT_VERSION,
            messages: self.messages.clone(),
            next_turn: self.next_turn,
            last_text: self.last_text.clone(),
            head_len: self.head_len,
            committed_turn_starts: self
                .committed_turn_starts
                .iter()
                .map(|&(turn, message_index)| TurnBoundary {
                    turn,
                    message_index,
                })
                .collect(),
            guard_history: self.guard_history.clone(),
            nudged_for_no_action: self.nudged_for_no_action,
            truncated_once: self.truncated_once,
            last_input_tokens: self.last_input_tokens,
            pending_input: self.pending_input.clone(),
            mutation_epoch: self.mutation_epoch,
            passed_checks: self.passed_checks.clone(),
        }
    }
}

pub(crate) fn completion_gate_message(
    mutation_epoch: u64,
    required_checks: &[String],
    fresh_checks: &[String],
) -> String {
    let missing = required_checks
        .iter()
        .filter(|name| !fresh_checks.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    format!(
        "Completion is blocked: run the required verification check(s) at workspace mutation epoch {mutation_epoch}: {}. Then call task_complete again.",
        missing.join(", ")
    )
}
