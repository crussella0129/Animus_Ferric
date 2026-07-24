use std::time::Duration;

use ferric_core::{ActionProtocol, FerricError, MediaPart, Message, RunPolicy};
use ferric_guard::Workspace;
use ferric_provider::{
    CompletionRequest, Constraint, Provider, SamplingParams, StreamDelta, ToolDescriptor,
};
use ferric_tools::{CheckRecord, ExecuteOutcome, Registry};
use ferric_trace::{Event, JsonlSink};
use tracing::{debug, info, instrument, warn};

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

/// A mutating tool call surfaced to the human for approval before it runs
/// (accept-edits mode, ADR-070). Carries what the model wants to do so the
/// caller's approver can render a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPreview {
    /// The tool being called, e.g. `write_file`.
    pub tool: String,
    /// The declared target path(s), if the call names any.
    pub targets: Vec<String>,
    /// A human-readable rendering of the call's arguments (the content to be
    /// written, the diff, …).
    pub detail: String,
}

/// Approves or rejects a pending mutating call. Returns `true` to let it run.
/// `Sync` so it can be held across the loop's `await` points, mirroring
/// `stream_sink`.
pub type EditApprover<'a> = &'a (dyn Fn(&EditPreview) -> bool + Sync);

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
    pub hooks: Option<ferric_core::HooksConfig>,
    /// Accept-edits mode (ADR-070): when set, each mutating (`Write`/`Execute`)
    /// tool call is previewed to this approver before it runs; a `false` verdict
    /// skips the call and reports a rejection to the model. `None` = run all
    /// (the default, unchanged behavior).
    pub edit_approver: Option<EditApprover<'a>>,
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
    #[allow(dead_code)]
    pub registry_tools: Vec<ToolDescriptor>,
    pub repetition: crate::repetition::RepetitionGuard,
    pub progress: crate::progress::ProgressGuard,
    pub failure: crate::failure::FailureGuard,
    pub nudged_for_no_action: bool,
    pub truncated_once: bool,
    pub last_input_tokens: Option<u32>,
}

impl<'a> LoopState<'a> {
    #[allow(clippy::collapsible_if)]
    #[instrument(level = "debug", name = "turn", skip_all, fields(turn = self.turns))]
    pub async fn step(&mut self) -> Result<TurnOutcome, FerricError> {
        if let Some(cancel) = &self.args.cancel_flag {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                warn!("interrupt observed; stopping the loop");
                return Ok(TurnOutcome::Stop(StopReason::Interrupted));
            }
        }

        if self.turns >= u32::from(self.args.policy.max_turns) {
            info!(
                max_turns = self.args.policy.max_turns,
                "turn budget exhausted; stopping"
            );
            return Ok(TurnOutcome::Stop(StopReason::MaxTurns));
        }

        if let Some(hooks) = &self.args.hooks {
            if let Some(cmd) = &hooks.pre_turn {
                if let Err(e) = crate::hooks_exec::run_hook(cmd, self.args.workspace.root()) {
                    warn!(error = %e, "pre_turn hook failed; stopping");
                    let note = Event::Note {
                        text: format!("pre_turn hook failed: {e}"),
                    };
                    self.sink.write_event(note.clone())?;
                    self.projector.step(&note);
                    return Ok(TurnOutcome::Stop(StopReason::HookFailed));
                }
            }
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
        .await?
        {
            debug!(
                input_tokens = self.last_input_tokens,
                "history compacted (budget threshold crossed)"
            );
            self.sink.write_event(event.clone())?;
            self.projector.step(&event);
        }

        let tools = match self.args.protocol {
            ActionProtocol::NativeTools => self.native_tools.clone(),
            ActionProtocol::ConstrainedJson | ActionProtocol::TextXml | ActionProtocol::Plan => {
                Vec::new()
            }
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
            chars: self
                .projector
                .messages
                .iter()
                .map(|m| m.text.as_deref().unwrap_or_default().len() as u64)
                .sum(),
            offered_tools: self.offered_names.clone(),
        };
        self.sink.write_event(assembled.clone())?;
        self.projector.step(&assembled);

        if self.args.protocol == ActionProtocol::ConstrainedJson
            || self.args.protocol == ActionProtocol::Plan
        {
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
                crate::backoff::complete_with_backoff(
                    self.args.provider,
                    request,
                    self.args.sleeper,
                    self.args.cancel_flag.clone(),
                )
                .await
            }
        };

        let completion = match completion_result {
            Ok(completion) => completion,
            Err(e) => {
                warn!(error = %e, "provider error after retries; stopping");
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

        let vcs = ferric_vcs::Vcs::new(self.args.workspace.root());
        if let Err(e) = vcs.snapshot(self.sink.session(), turn).await {
            // debug, not warn: in a non-git workspace this fires every turn, so
            // a WARN would break quiet-by-default. The failure is still recorded
            // as a trace Note below (the durable record); revert is simply
            // unavailable for this turn.
            debug!(error = %e, turn, "vcs snapshot failed (revert unavailable for this turn)");
            self.sink.write_event(Event::Note {
                text: format!("vcs snapshot failed: {e}"),
            })?;
        }

        self.last_input_tokens = completion.input_tokens;

        if (self.args.protocol == ActionProtocol::ConstrainedJson
            || self.args.protocol == ActionProtocol::Plan)
            && completion.truncated
        {
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
                if let Some(hooks) = &self.args.hooks {
                    if let Some(cmd) = &hooks.post_turn {
                        if let Err(e) = crate::hooks_exec::run_hook(cmd, self.args.workspace.root())
                        {
                            let note = Event::Note {
                                text: format!("post_turn hook failed: {e}"),
                            };
                            self.sink.write_event(note.clone())?;
                            self.projector.step(&note);
                        }
                    }
                }
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
                warn!(guard = "repetition", "identical action repeated; stopping");
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
                warn!(
                    guard = "no_progress",
                    "same tool with churning args; stopping"
                );
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

            // Accept-edits gate (ADR-070): a mutating call is previewed to the
            // human, who may reject it before it touches disk. Non-mutating
            // (Read) calls, and runs with no approver, are never gated.
            if let Some(approver) = self.args.edit_approver {
                let mutating = matches!(
                    self.args.registry.permission_of(&call.name),
                    Some(ferric_guard::PermissionLevel::Write)
                        | Some(ferric_guard::PermissionLevel::Execute)
                );
                if mutating && !approver(&edit_preview(&call.name, &call.args)) {
                    warn!(tool = %call.name, "edit rejected by user (accept-edits)");
                    let tr = Event::ToolResult {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        output: "edit rejected by user".to_string(),
                        is_error: true,
                        duration_ms: 0,
                    };
                    self.sink.write_event(tr.clone())?;
                    self.projector.step(&tr);
                    dispatched += 1;
                    errored += 1;
                    continue;
                }
            }

            debug!(tool = %call.name, "dispatching tool call");
            let (result_text, is_error, duration_ms, checks) = dispatch(
                self.args.registry,
                self.args.workspace,
                &call.name,
                &call.args,
                &self.args.taint_set,
                &self.args.sink_policy,
            );
            debug!(tool = %call.name, is_error, duration_ms, "tool call finished");
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
            if let Some(stream_sink) = self.args.stream_sink {
                // Determine a short summary (first line) for the stream display.
                let summary_line = result_text.full.lines().next().unwrap_or("").to_string();
                let summary = if is_error {
                    format!("Error: {}", summary_line)
                } else {
                    summary_line
                };
                stream_sink(StreamDelta::ToolCompleted {
                    name: call.name.clone(),
                    summary,
                });
            }
        }

        if let Some(summary) = terminate_with {
            self.projector.commit_pending();
            self.projector.last_text = Some(summary);

            if let Some(hooks) = &self.args.hooks {
                if let Some(cmd) = &hooks.post_turn {
                    if let Err(e) = crate::hooks_exec::run_hook(cmd, self.args.workspace.root()) {
                        let note = Event::Note {
                            text: format!("post_turn hook failed: {e}"),
                        };
                        self.sink.write_event(note.clone())?;
                        self.projector.step(&note);
                    }
                }
            }

            return Ok(TurnOutcome::Stop(StopReason::TaskComplete));
        }

        if let Some(plan) = plan_terminate_with {
            self.projector.commit_pending();
            self.projector.last_text = Some(plan);

            if let Some(hooks) = &self.args.hooks {
                if let Some(cmd) = &hooks.post_turn {
                    if let Err(e) = crate::hooks_exec::run_hook(cmd, self.args.workspace.root()) {
                        let note = Event::Note {
                            text: format!("post_turn hook failed: {e}"),
                        };
                        self.sink.write_event(note.clone())?;
                        self.projector.step(&note);
                    }
                }
            }

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
                    warn!(
                        guard = "failure",
                        dispatched, errored, "every tool call erroring; stopping"
                    );
                    let evt = Event::FailureGuard {
                        action: "stopped".to_string(),
                    };
                    self.sink.write_event(evt.clone())?;
                    self.projector.step(&evt);
                    return Ok(TurnOutcome::Stop(StopReason::RepeatedFailure));
                }
            }
        }
        if let Some(hooks) = &self.args.hooks {
            if let Some(cmd) = &hooks.post_turn {
                if let Err(e) = crate::hooks_exec::run_hook(cmd, self.args.workspace.root()) {
                    let note = Event::Note {
                        text: format!("post_turn hook failed: {e}"),
                    };
                    self.sink.write_event(note.clone())?;
                    self.projector.step(&note);
                }
            }
        }

        Ok(TurnOutcome::Continue)
    }
}

#[allow(clippy::collapsible_if)]
pub async fn run(
    args: RunArgs<'_>,
    sink: &mut JsonlSink,
    prompt: Option<&str>,
) -> Result<LoopOutcome, FerricError> {
    // Keep the projector's model-facing cap in step with the registry's, so a
    // caller-configured `Registry::with_truncation_limit` actually reaches the
    // context window.
    let mut projector =
        TraceProjector::new().with_truncation_limit(args.registry.truncation_limit());

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
            let mut system = args
                .system_prompt
                .unwrap_or(DEFAULT_SYSTEM_PROMPT)
                .to_string();
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

    info!(reason = stop.as_str(), turns = state.turns, "loop finished");

    let session_end = Event::SessionEnd {
        reason: stop.as_str().to_string(),
    };
    state.sink.write_event(session_end.clone())?;
    state.projector.step(&session_end);

    state.projector.commit_pending();

    let is_error = !matches!(
        stop,
        StopReason::TaskComplete | StopReason::PlanSubmitted | StopReason::FinalText
    );
    if is_error {
        if let Some(hooks) = &state.args.hooks {
            if let Some(cmd) = &hooks.on_error {
                if let Err(e) = crate::hooks_exec::run_hook(cmd, state.args.workspace.root()) {
                    let note = Event::Note {
                        text: format!("on_error hook failed: {e}"),
                    };
                    let _ = state.sink.write_event(note.clone());
                    state.projector.step(&note);
                }
            }
        }
    }

    Ok(LoopOutcome {
        final_text: state.projector.last_text,
        stop,
        turns: state.turns,
    })
}

fn registry_tools(
    registry: &Registry,
    policy: &RunPolicy,
    protocol: ActionProtocol,
) -> Vec<ToolDescriptor> {
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

/// Build an [`EditPreview`] from a pending call: pull the target `path` (if the
/// call names one) and render the arguments for the human to inspect.
fn edit_preview(name: &str, args: &serde_json::Value) -> EditPreview {
    let mut targets = Vec::new();
    for key in ["path", "from", "to", "src", "dest"] {
        if let Some(s) = args.get(key).and_then(|v| v.as_str()) {
            targets.push(s.to_string());
        }
    }
    let detail = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
    EditPreview {
        tool: name.to_string(),
        targets,
        detail,
    }
}

/// The full, untruncated tool output. It goes to the trace verbatim (ADR-002
/// durability); the model-facing truncation is applied by the projector, which
/// is the single place the context window is assembled.
struct DispatchText {
    full: String,
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
            DispatchText { full: output.full },
            output.is_error,
            duration_ms,
            checks,
        ),
        ExecuteOutcome::Denied { reason, checks } => {
            let text = format!("DENIED: {reason}");
            (DispatchText { full: text }, true, 0, checks)
        }
        ExecuteOutcome::UnknownTool { name } => {
            let text = format!("unknown tool: {name}");
            (DispatchText { full: text }, true, 0, Vec::new())
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
