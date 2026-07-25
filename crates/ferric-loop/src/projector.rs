use ferric_core::{ActionProtocol, Message, Role, ToolCall};
use ferric_trace::Event;

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
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<(String, String, String)>,
    pub repetition_warned: bool,
    pub no_progress_warned: bool,
    pub failure_warned: bool,
    pub oscillation_warned: bool,
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
        if self.oscillation_warned {
            out.push(oscillation_warn_message());
        }
        for (id, name, output) in &self.tool_results {
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
    /// Model-facing cap on a single tool result (ADR-002). Kept here rather than
    /// read from the registry because the projector must also work in replay,
    /// where there is no registry — only the trace.
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
            truncation_limit: ferric_tools::DEFAULT_TRUNCATION_LIMIT,
        }
    }

    /// Match a registry configured with a non-default cap
    /// (`Registry::with_truncation_limit`).
    pub fn with_truncation_limit(mut self, limit: usize) -> Self {
        self.truncation_limit = limit;
        self
    }

    /// Feeds an event into the projector, updating the context window.
    pub fn step(&mut self, event: &Event) {
        match event {
            Event::PolicySelected { protocol, .. } => self.protocol = Some(*protocol),
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
            Event::TurnStart { turn } => {
                self.commit_pending();
                self.pending = Some(PendingTurn {
                    turn: *turn,
                    ..Default::default()
                });
            }
            Event::TurnEnd {
                text, truncated, ..
            } => {
                if let Some(p) = &mut self.pending {
                    p.turn_end = Some((text.clone(), *truncated));
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
                id, name, output, ..
            } => {
                if let Some(p) = &mut self.pending {
                    p.tool_results
                        .push((id.clone(), name.clone(), output.clone()));
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
                    let preserved_tail: Vec<Message> = self.messages[preserve_from_idx..].to_vec();
                    self.messages.truncate(self.head_len);
                    self.messages
                        .push(Message::user(format!("[compacted history] {summary}")));
                    let new_base = self.messages.len();
                    self.messages.extend(preserved_tail);
                    let shift = preserve_from_idx - new_base;
                    self.committed_turn_starts = self.committed_turn_starts[split_at..]
                        .iter()
                        .map(|&(t, idx)| (t, idx - shift))
                        .collect();
                }
            }
            _ => {}
        }
    }

    /// Commits the currently open pending turn (a later TurnStart confirms it
    /// finished dispatching) and appends its messages to the context window.
    pub fn commit_pending(&mut self) {
        if let (Some(p), Some(proto)) = (self.pending.take(), self.protocol) {
            let turn_num = p.turn;
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
            }
        }
    }
}
