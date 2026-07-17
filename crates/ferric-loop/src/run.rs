use std::time::Duration;

use ferric_core::{ActionProtocol, FerricError, MediaPart, Message, RunPolicy};
use ferric_guard::Workspace;
use ferric_provider::{
    CompletionRequest, Constraint, Provider, SamplingParams, StreamDelta, ToolDescriptor,
};
use ferric_tools::{CheckRecord, ExecuteOutcome, Registry};
use ferric_trace::{Event, JsonlSink};

use crate::outcome::{LoopOutcome, StopReason};
use crate::projector::TraceProjector;

/// Injectable clock for backoff.
pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration);
}

pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Ferric, a coding agent. \
Use the available tools to act on the workspace. \
For each tool call, write out your reasoning in the `thought` field before executing the tool. \
When the task is done, call task_complete with a one-sentence summary. \
Never describe a tool call in prose - actually call the tool.";

pub type PromptLineage = (String, String, Vec<(String, String)>);

pub struct RunArgs<'a> {
    pub provider: &'a dyn Provider,
    pub registry: &'a Registry,
    pub workspace: &'a Workspace,
    pub policy: &'a RunPolicy,
    pub protocol: ActionProtocol,
    pub sampling: SamplingParams,
    pub sleeper: &'a dyn Sleeper,
    pub system_prompt: Option<&'a str>,
    pub prompt_lineage: Option<PromptLineage>,
    pub media: Vec<MediaPart>,
    pub stream_sink: Option<&'a (dyn Fn(StreamDelta) + Sync)>,
    pub resume: Option<crate::replay::ReplayedState>,
    pub cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub taint_set: ferric_guard::TaintSet,
    pub sink_policy: ferric_guard::SinkPolicy,
}

pub enum TurnOutcome {
    Continue,
    Stop(StopReason),
}

pub struct LoopState<'a> {
    pub args: RunArgs<'a>,
    pub sink: &'a mut JsonlSink,
    pub projector: TraceProjector,
    pub turns: u32,
    pub offered_names: Vec<String>,
    pub native_tools: Vec<ToolDescriptor>,
    pub registry_tools: Vec<ToolDescriptor>,
    pub repetition: crate::repetition::RepetitionGuard,
    pub progress: crate::progress::ProgressGuard,
    pub failure: crate::failure::FailureGuard,
    pub nudged_for_no_action: bool,
    pub truncated_once: bool,
    pub last_input_tokens: Option<u32>,
}

impl<'a> LoopState<'a> {
    pub async fn step(&mut self) -> Result<TurnOutcome, FerricError> {
        if let Some(cancel) = &self.args.cancel_flag {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(TurnOutcome::Stop(StopReason::Interrupted));
            }
        }

        if self.turns >= u32::from(self.args.policy.max_turns) {
            return Ok(TurnOutcome::Stop(StopReason::MaxTurns));
        }
        let turn = self.turns;
        self.turns += 1;
        let start_event = Event::TurnStart { turn };
        self.sink.write_event(start_event.clone())?;
        self.projector.step(&start_event);

        if let Some(event) = crate::compact::maybe_compact(
            &self.projector,
            self.args.provider,
            self.args.sleeper,
            self.args.policy,
            self.last_input_tokens,
            self.args.cancel_flag.clone(),
        )
        .await? {
            self.sink.write_event(event.clone())?;
            self.projector.step(&event);
        }

        let tools = match self.args.protocol {
            ActionProtocol::NativeTools => self.native_tools.clone(),
            ActionProtocol::ConstrainedJson | ActionProtocol::TextXml | ActionProtocol::Plan => Vec::new(),
        };

        let constraint = match self.args.protocol {
            ActionProtocol::ConstrainedJson | ActionProtocol::Plan => Some(Constraint::JsonSchema(
                crate::grammar::action_schema(&self.native_tools),
            )),
            ActionProtocol::NativeTools | ActionProtocol::TextXml => None,
        };
        
        let request = CompletionRequest {
            messages: self.projector.messages.clone(),
            sampling: self.args.sampling.clone(),
            tools,
            constraint,
        };
        
        if let Err(e) = request.validate() {
            let note = Event::Note {
                text: format!("invalid request constructed by loop: {e}"),
            };
            self.sink.write_event(note.clone())?;
            self.projector.step(&note);
            return Ok(TurnOutcome::Stop(StopReason::ProviderError));
        }
        
        let assembled = Event::PromptAssembled {
            turn,
            message_count: self.projector.messages.len() as u32,
            chars: self.projector.messages
                .iter()
                .map(|m| m.text.as_deref().unwrap_or_default().len() as u64)
                .sum(),
            offered_tools: self.offered_names.clone(),
        };
        self.sink.write_event(assembled.clone())?;
        self.projector.step(&assembled);

        if self.args.protocol == ActionProtocol::ConstrainedJson || self.args.protocol == ActionProtocol::Plan {
            let evt = Event::ConstraintApplied {
                kind: "json_schema".to_string(),
            };
            self.sink.write_event(evt.clone())?;
            self.projector.step(&evt);
        }

        let completion_result = match self.args.stream_sink {
            Some(on_delta) => {
                crate::backoff::complete_streaming_with_backoff(
                    self.args.provider,
                    request,
                    self.args.sleeper,
                    on_delta,
                    self.args.cancel_flag.clone(),
                )
                .await
            }
            None => {
                crate::backoff::complete_with_backoff(self.args.provider, request, self.args.sleeper, self.args.cancel_flag.clone()).await
            }
        };
        
        let completion = match completion_result {
            Ok(completion) => completion,
            Err(e) => {
                self.sink.write_event(Event::Note {
                    text: format!("provider error: {e}"),
                })?;
                return Ok(TurnOutcome::Stop(StopReason::ProviderError));
            }
        };

        let turn_end = Event::TurnEnd {
            turn,
            text: completion.message.text.clone(),
            tool_call_count: completion.message.tool_calls.len() as u32,
            input_tokens: completion.input_tokens,
            output_tokens: completion.output_tokens,
            truncated: completion.truncated,
        };
        self.sink.write_event(turn_end.clone())?;
        self.projector.step(&turn_end);
        
        self.last_input_tokens = completion.input_tokens;

        if (self.args.protocol == ActionProtocol::ConstrainedJson || self.args.protocol == ActionProtocol::Plan) && completion.truncated {
            if self.truncated_once {
                return Ok(TurnOutcome::Stop(StopReason::TruncatedAction));
            }
            self.truncated_once = true;
            return Ok(TurnOutcome::Continue);
        }

        let (actions, _parse_error) = match self.args.protocol {
            ActionProtocol::NativeTools => (completion.message.tool_calls.clone(), None),
            ActionProtocol::ConstrainedJson | ActionProtocol::Plan => {
                match crate::grammar::parse_json_action(
                    turn,
                    completion.message.text.as_deref().unwrap_or_default(),
                ) {
                    Ok(call) => (vec![call], None),
                    Err(e) => (Vec::new(), Some(e)),
                }
            }
            ActionProtocol::TextXml => {
                match crate::grammar::parse_action(
                    turn,
                    completion.message.text.as_deref().unwrap_or_default(),
                ) {
                    Ok(call) => (vec![call], None),
                    Err(e) => (Vec::new(), Some(e)),
                }
            }
        };

        if actions.is_empty() {
            let is_native_final = self.args.protocol == ActionProtocol::NativeTools
                && completion
                    .message
                    .text
                    .as_deref()
                    .is_some_and(|t| !t.trim().is_empty());
            if is_native_final {
                return Ok(TurnOutcome::Stop(StopReason::FinalText));
            }
            if self.nudged_for_no_action {
                return Ok(TurnOutcome::Stop(StopReason::EmptyCompletion));
            }
            self.nudged_for_no_action = true;
            return Ok(TurnOutcome::Continue);
        }

        match self.repetition.observe(&actions) {
            crate::repetition::Verdict::Proceed => {}
            crate::repetition::Verdict::Warn => {
                let evt = Event::RepetitionGuard {
                    action: "warned".to_string(),
                };
                self.sink.write_event(evt.clone())?;
                self.projector.step(&evt);
            }
            crate::repetition::Verdict::Stop => {
                let evt = Event::RepetitionGuard {
                    action: "stopped".to_string(),
                };
                self.sink.write_event(evt.clone())?;
                self.projector.step(&evt);
                return Ok(TurnOutcome::Stop(StopReason::RepetitionGuard));
            }
        }

        match self.progress.observe(&actions) {
            crate::repetition::Verdict::Proceed => {}
            crate::repetition::Verdict::Warn => {
                let evt = Event::NoProgressGuard {
                    action: "warned".to_string(),
                };
                self.sink.write_event(evt.clone())?;
                self.projector.step(&evt);
            }
            crate::repetition::Verdict::Stop => {
                let evt = Event::NoProgressGuard {
                    action: "stopped".to_string(),
                };
                self.sink.write_event(evt.clone())?;
                self.projector.step(&evt);
                return Ok(TurnOutcome::Stop(StopReason::NoProgress));
            }
        }

        let mut terminate_with: Option<String> = None;
        let mut plan_terminate_with: Option<String> = None;
        let mut dispatched = 0usize;
        let mut errored = 0usize;
        for call in &actions {
            if crate::terminator::is_task_complete(&call.name) {
                terminate_with = Some(crate::terminator::summary_of(&call.args));
                let tc = Event::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    args: call.args.clone(),
                };
                self.sink.write_event(tc.clone())?;
                self.projector.step(&tc);
                continue;
            }
            if crate::terminator::is_submit_plan(&call.name) {
                plan_terminate_with = Some(crate::terminator::plan_of(&call.args));
                let tc = Event::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    args: call.args.clone(),
                };
                self.sink.write_event(tc.clone())?;
                self.projector.step(&tc);
                continue;
            }
            let tc = Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            };
            self.sink.write_event(tc.clone())?;
            self.projector.step(&tc);
            
            let (result_text, is_error, duration_ms, checks) = dispatch(
                self.args.registry,
                self.args.workspace,
                &call.name,
                &call.args,
                &self.args.taint_set,
                &self.args.sink_policy,
            );
            dispatched += 1;
            if is_error {
                errored += 1;
            }
            for check in &checks {
                let evt = permission_event(check);
                self.sink.write_event(evt.clone())?;
                self.projector.step(&evt);
            }
            let tr = Event::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                output: result_text.full.clone(),
                is_error,
                duration_ms,
            };
            self.sink.write_event(tr.clone())?;
            self.projector.step(&tr);
        }
        
        if let Some(summary) = terminate_with {
            self.projector.commit_pending();
            self.projector.last_text = Some(summary);
            return Ok(TurnOutcome::Stop(StopReason::TaskComplete));
        }
        
        if let Some(plan) = plan_terminate_with {
            self.projector.commit_pending();
            self.projector.last_text = Some(plan);
            return Ok(TurnOutcome::Stop(StopReason::PlanSubmitted));
        }

        if dispatched > 0 {
            match self.failure.observe_turn(dispatched, errored) {
                crate::repetition::Verdict::Proceed => {}
                crate::repetition::Verdict::Warn => {
                    let evt = Event::FailureGuard {
                        action: "warned".to_string(),
                    };
                    self.sink.write_event(evt.clone())?;
                    self.projector.step(&evt);
                }
                crate::repetition::Verdict::Stop => {
                    let evt = Event::FailureGuard {
                        action: "stopped".to_string(),
                    };
                    self.sink.write_event(evt.clone())?;
                    self.projector.step(&evt);
                    return Ok(TurnOutcome::Stop(StopReason::RepeatedFailure));
                }
            }
        }
        
        Ok(TurnOutcome::Continue)
    }
}

pub async fn run(
    args: RunArgs<'_>,
    sink: &mut JsonlSink,
    prompt: Option<&str>,
) -> Result<LoopOutcome, FerricError> {
    let mut projector = TraceProjector::new();

    let session_start = Event::SessionStart {
        workspace: args.workspace.root().display().to_string(),
        resumed_from: args.resume.as_ref().map(|r| r.source_session.clone()),
    };
    sink.write_event(session_start.clone())?;
    projector.step(&session_start);

    let policy_selected = Event::PolicySelected {
        tier: args.policy.tier,
        protocol: args.protocol,
        max_turns: u32::from(args.policy.max_turns),
        max_tools: u32::from(args.policy.max_tools),
        prompt_budget_tokens: args.policy.prompt_budget_tokens,
        max_output_tokens: args.policy.max_output_tokens,
    };
    sink.write_event(policy_selected.clone())?;
    projector.step(&policy_selected);

    if let Some((output_id, output_version, composed_of)) = &args.prompt_lineage {
        let composed = Event::PromptComposed {
            output_id: output_id.clone(),
            output_version: output_version.clone(),
            composed_of: composed_of.clone(),
        };
        sink.write_event(composed.clone())?;
        projector.step(&composed);
    }

    let registry_tools = registry_tools(args.registry, args.policy, args.protocol);

    let turns = match &args.resume {
        Some(replayed) => {
            projector.messages = replayed.messages.clone();
            projector.turns = replayed.turns;
            projector.last_text = replayed.last_text.clone();
            projector.protocol = Some(replayed.protocol);
            projector.head_len = replayed.messages.len();
            replayed.turns
        }
        None => {
            let mut system = args.system_prompt.unwrap_or(DEFAULT_SYSTEM_PROMPT).to_string();
            if args.system_prompt.is_none() {
                system.push_str("\n\nAvailable tools:\n");
                for t in &registry_tools {
                    system.push_str(&format!("- {}: {}\n", t.name, t.description));
                }
                system.push_str("- task_complete: Finish the task and provide a summary.\n");
            }
            
            let prompt_text = prompt.ok_or_else(|| {
                FerricError::InvalidInput(
                    "run() requires a prompt when not resuming a session".to_string(),
                )
            })?;
            let session_prompt = Event::SessionPrompt {
                system,
                user: prompt_text.to_string(),
                media: args.media.clone(),
            };
            sink.write_event(session_prompt.clone())?;
            projector.step(&session_prompt);
            0
        }
    };

    if args.resume.is_some()
        && let Some(extra) = prompt
    {
        projector.messages.push(Message::user(extra));
    }

    let mut offered_names: Vec<String> = registry_tools.iter().map(|t| t.name.clone()).collect();
    if args.protocol == ActionProtocol::Plan {
        offered_names.push(crate::terminator::SUBMIT_PLAN.to_string());
    } else {
        offered_names.push(crate::terminator::TASK_COMPLETE.to_string());
    }

    let native_tools: Vec<ToolDescriptor> = {
        let mut v = registry_tools.clone();
        if args.protocol == ActionProtocol::Plan {
            v.push(crate::terminator::plan_descriptor());
        } else {
            v.push(crate::terminator::descriptor());
        }
        v
    };

    let mut state = LoopState {
        args,
        sink,
        projector,
        turns,
        offered_names,
        native_tools,
        registry_tools,
        repetition: crate::repetition::RepetitionGuard::new(),
        progress: crate::progress::ProgressGuard::new(),
        failure: crate::failure::FailureGuard::new(),
        nudged_for_no_action: false,
        truncated_once: false,
        last_input_tokens: None,
    };

    let stop = loop {
        match state.step().await? {
            TurnOutcome::Continue => {}
            TurnOutcome::Stop(reason) => break reason,
        }
    };

    let session_end = Event::SessionEnd {
        reason: stop.as_str().to_string(),
    };
    state.sink.write_event(session_end.clone())?;
    state.projector.step(&session_end);
    
    state.projector.commit_pending();

    Ok(LoopOutcome {
        final_text: state.projector.last_text,
        stop,
        turns: state.turns,
    })
}


fn registry_tools(registry: &Registry, policy: &RunPolicy, protocol: ActionProtocol) -> Vec<ToolDescriptor> {
    registry
        .tools_for_policy(policy)
        .into_iter()
        .filter(|spec| {
            if protocol == ActionProtocol::Plan {
                spec.permission == ferric_guard::PermissionLevel::Read
            } else {
                true
            }
        })
        .map(|spec| ToolDescriptor {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        })
        .collect()
}

struct DispatchText {
    full: String,
    _for_model: String,
}

fn dispatch(
    registry: &Registry,
    workspace: &Workspace,
    name: &str,
    args: &serde_json::Value,
    taint_set: &ferric_guard::TaintSet,
    sink_policy: &ferric_guard::SinkPolicy,
) -> (DispatchText, bool, u64, Vec<CheckRecord>) {
    match registry.execute(workspace, name, args, taint_set, sink_policy) {
        ExecuteOutcome::Completed {
            output,
            duration_ms,
            checks,
        } => (
            DispatchText {
                full: output.full,
                _for_model: output.for_model,
            },
            output.is_error,
            duration_ms,
            checks,
        ),
        ExecuteOutcome::Denied { reason, checks } => {
            let text = format!("DENIED: {reason}");
            (
                DispatchText {
                    full: text.clone(),
                    _for_model: text,
                },
                true,
                0,
                checks,
            )
        }
        ExecuteOutcome::UnknownTool { name } => {
            let text = format!("unknown tool: {name}");
            (
                DispatchText {
                    full: text.clone(),
                    _for_model: text,
                },
                true,
                0,
                Vec::new(),
            )
        }
    }
}

fn permission_event(check: &CheckRecord) -> Event {
    Event::PermissionCheck {
        path: check.path.display().to_string(),
        decision: check.decision.clone(),
        rule: check.rule.clone(),
        matched: check.matched.clone(),
    }
}
