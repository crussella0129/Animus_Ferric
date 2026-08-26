use std::time::Duration;

use ferric_core::{
    ActionProtocol, FerricError, HarnessPolicy, MediaPart, RunPolicy, UserInputRequest,
};
use ferric_guard::Workspace;
use ferric_provider::{
    CompletionRequest, Constraint, Provider, SamplingParams, StreamDelta, ToolDescriptor,
};
use ferric_tools::{CheckRecord, ExecuteOutcome, Registry};
use ferric_trace::{Event, JsonlSink};
use tracing::{debug, info, instrument, warn};

use crate::ControllerState;
use crate::outcome::{LoopOutcome, NeedsInput, StopReason};
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

const GENERAL_EVIDENCE_GUIDANCE_V1: &str = "[Ferric general evidence guidance v1]\n\
Inspect a complete current file in a prior turn before editing existing content; paginate incomplete reads until coverage is complete.\n\
Genuinely absent paths may be created directly without a prior file read.\n\
After a failed check, inspect relevant repository evidence in a later turn before attempting a repair.\n\
Do not rerun the same named check until a material workspace mutation advances the evidence epoch.";

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
    /// Operator-requested harness policy. `None` means Legacy for a fresh run
    /// and inherits the recorded policy for a resumed run.
    pub harness_policy: Option<HarnessPolicy>,
    pub sampling: SamplingParams,
    pub sleeper: &'a dyn Sleeper,
    pub system_prompt: Option<&'a str>,
    pub prompt_lineage: Option<PromptLineage>,
    pub media: Vec<MediaPart>,
    pub stream_sink: Option<&'a (dyn Fn(StreamDelta) + Sync)>,
    pub resume: Option<crate::replay::ReplayedState>,
    /// Explicit answer to a pending `request_user_input` pause. Only valid
    /// together with `resume`; it is intentionally distinct from a generic
    /// goal-amendment prompt.
    pub answer: Option<&'a str>,
    pub cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub provenance: ferric_guard::Provenance,
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
    /// Prompt-independent evidence truth. `None` is mandatory for Legacy.
    pub controller: Option<ControllerState>,
    /// Absolute id assigned to the next turn across resume boundaries.
    pub turns: u32,
    /// Budget consumption in this process run. Resume always gets a fresh
    /// policy budget without reusing absolute turn ids.
    pub turns_this_run: u32,
    pub offered_names: Vec<String>,
    pub native_tools: Vec<ToolDescriptor>,
    pub repetition: crate::repetition::RepetitionGuard,
    pub progress: crate::progress::ProgressGuard,
    pub failure: crate::failure::FailureGuard,
    pub oscillation: crate::oscillation::OscillationGuard,
    pub nudged_for_no_action: bool,
    pub truncated_once: bool,
    pub last_input_tokens: Option<u32>,
}

impl<'a> LoopState<'a> {
    fn commit_turn(
        &mut self,
        turn: u32,
        dispatched: usize,
        errored: usize,
        stop: Option<StopReason>,
        snapshot_commit: Option<String>,
    ) -> Result<(), FerricError> {
        let committed = Event::TurnCommitted {
            turn,
            dispatched: dispatched as u32,
            errored: errored as u32,
            stop_reason: stop.map(|reason| reason.as_str().to_string()),
            snapshot_commit,
        };
        self.sink.write_event(committed.clone())?;
        self.projector.step(&committed);
        Ok(())
    }

    fn record_synthetic_result(
        &mut self,
        call: &ferric_core::ToolCall,
        output: String,
        is_error: bool,
    ) -> Result<(), FerricError> {
        let tc = Event::ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.args.clone(),
        };
        self.sink.write_event(tc.clone())?;
        self.projector.step(&tc);
        let tr = Event::ToolResult {
            id: call.id.clone(),
            name: call.name.clone(),
            output,
            is_error,
            duration_ms: 0,
        };
        self.sink.write_event(tr.clone())?;
        self.projector.step(&tr);
        Ok(())
    }

    fn completion_evidence(&self) -> (Vec<String>, Vec<String>) {
        if let Some(controller) = self.controller.as_ref() {
            let required = controller.required_checks().to_vec();
            let fresh = required
                .iter()
                .filter(|name| {
                    controller.passed_checks().get(*name) == Some(&controller.mutation_epoch())
                })
                .cloned()
                .collect();
            (required, fresh)
        } else {
            let required = self.args.registry.required_checks().to_vec();
            let fresh = required
                .iter()
                .filter(|name| {
                    self.projector.passed_checks.get(*name) == Some(&self.projector.mutation_epoch)
                })
                .cloned()
                .collect();
            (required, fresh)
        }
    }

    fn record_completion_gate(
        &mut self,
        required_checks: Vec<String>,
        fresh_checks: Vec<String>,
        passed: bool,
    ) -> Result<(), FerricError> {
        let mutation_epoch = self.controller.as_ref().map_or(
            self.projector.mutation_epoch,
            ControllerState::mutation_epoch,
        );
        let event = Event::CompletionGate {
            mutation_epoch,
            required_checks,
            fresh_checks,
            decision: if passed { "passed" } else { "blocked" }.to_string(),
        };
        self.sink.write_event(event.clone())?;
        self.projector.step(&event);
        Ok(())
    }

    #[allow(clippy::collapsible_if)]
    #[instrument(level = "debug", name = "turn", skip_all, fields(turn = self.turns))]
    pub async fn step(&mut self) -> Result<TurnOutcome, FerricError> {
        if let Some(cancel) = &self.args.cancel_flag {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                warn!("interrupt observed; stopping the loop");
                return Ok(TurnOutcome::Stop(StopReason::Interrupted));
            }
        }

        if self.turns_this_run >= u32::from(self.args.policy.max_turns) {
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
        self.turns_this_run += 1;
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
        let snapshot_commit = match vcs.snapshot(self.sink.session(), turn) {
            Ok(commit) => Some(commit),
            Err(e) => {
                // debug, not warn: in a non-git workspace this fires every
                // turn. The durable note still explains why revert is absent.
                debug!(error = %e, turn, "vcs snapshot failed (revert unavailable for this turn)");
                self.sink.write_event(Event::Note {
                    text: format!("vcs snapshot failed: {e}"),
                })?;
                None
            }
        };

        self.last_input_tokens = completion.input_tokens;

        if (self.args.protocol == ActionProtocol::ConstrainedJson
            || self.args.protocol == ActionProtocol::Plan)
            && completion.truncated
        {
            let proposed = Event::ActionsProposed {
                turn,
                calls: Vec::new(),
            };
            self.sink.write_event(proposed.clone())?;
            self.projector.step(&proposed);
            if self.truncated_once {
                self.commit_turn(
                    turn,
                    0,
                    0,
                    Some(StopReason::TruncatedAction),
                    snapshot_commit,
                )?;
                return Ok(TurnOutcome::Stop(StopReason::TruncatedAction));
            }
            self.truncated_once = true;
            self.commit_turn(turn, 0, 0, None, snapshot_commit)?;
            return Ok(TurnOutcome::Continue);
        }

        let (actions, parse_error) = match self.args.protocol {
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

        let proposed = Event::ActionsProposed {
            turn,
            calls: actions.clone(),
        };
        self.sink.write_event(proposed.clone())?;
        self.projector.step(&proposed);

        if actions.is_empty() {
            // Record WHY there was no action. Without this a grammar failure and
            // a genuinely empty completion are indistinguishable in the trace,
            // which makes post-hoc analysis of small-model behaviour guesswork.
            if let Some(e) = &parse_error {
                debug!(error = %e, "action parse failed");
                let note = Event::Note {
                    text: format!("action parse failed: {e}"),
                };
                self.sink.write_event(note.clone())?;
                self.projector.step(&note);
            }

            let is_native_final = self.args.protocol == ActionProtocol::NativeTools
                && completion
                    .message
                    .text
                    .as_deref()
                    .is_some_and(|t| !t.trim().is_empty());
            if is_native_final {
                let (required, fresh) = self.completion_evidence();
                if !required.is_empty() {
                    let passed = required.len() == fresh.len();
                    self.record_completion_gate(required, fresh, passed)?;
                    if !passed {
                        self.commit_turn(turn, 0, 0, None, snapshot_commit)?;
                        self.fire_post_turn()?;
                        return Ok(TurnOutcome::Continue);
                    }
                }
                self.commit_turn(turn, 0, 0, Some(StopReason::FinalText), snapshot_commit)?;
                self.fire_post_turn()?;
                return Ok(TurnOutcome::Stop(StopReason::FinalText));
            }
            if self.nudged_for_no_action {
                self.commit_turn(
                    turn,
                    0,
                    0,
                    Some(StopReason::EmptyCompletion),
                    snapshot_commit,
                )?;
                return Ok(TurnOutcome::Stop(StopReason::EmptyCompletion));
            }
            self.nudged_for_no_action = true;
            self.commit_turn(turn, 0, 0, None, snapshot_commit)?;
            return Ok(TurnOutcome::Continue);
        }

        // Clarification is a control transition, not an executable tool. It
        // must be intercepted before any action-shape guard so the act of
        // declaring ambiguity cannot be mistaken for repetition or flailing.
        if actions
            .iter()
            .any(|call| crate::terminator::is_request_user_input(&call.name))
        {
            if actions.len() != 1 {
                let output = "request_user_input must be the only action in its turn; no calls were executed"
                    .to_string();
                for call in &actions {
                    self.record_synthetic_result(call, output.clone(), true)?;
                }
                let dispatched = actions.len();
                let errored = dispatched;
                let failure_stop = match self.failure.observe_turn(dispatched, errored) {
                    crate::repetition::Verdict::Proceed => None,
                    crate::repetition::Verdict::Warn => {
                        let event = Event::FailureGuard {
                            action: "warned".to_string(),
                        };
                        self.sink.write_event(event.clone())?;
                        self.projector.step(&event);
                        None
                    }
                    crate::repetition::Verdict::Stop => {
                        let event = Event::FailureGuard {
                            action: "stopped".to_string(),
                        };
                        self.sink.write_event(event.clone())?;
                        self.projector.step(&event);
                        Some(StopReason::RepeatedFailure)
                    }
                };
                self.commit_turn(turn, dispatched, errored, failure_stop, snapshot_commit)?;
                self.fire_post_turn()?;
                return Ok(failure_stop.map_or(TurnOutcome::Continue, TurnOutcome::Stop));
            }

            let call = &actions[0];
            match crate::terminator::request_of(&call.args) {
                Ok(request) => {
                    self.record_synthetic_result(
                        call,
                        "user input requested; session paused until an answer is supplied"
                            .to_string(),
                        false,
                    )?;
                    self.projector.pending_input = Some(request);
                    self.commit_turn(turn, 1, 0, Some(StopReason::NeedsInput), snapshot_commit)?;
                    self.fire_post_turn()?;
                    return Ok(TurnOutcome::Stop(StopReason::NeedsInput));
                }
                Err(error) => {
                    self.record_synthetic_result(call, error.to_string(), true)?;
                    let failure_stop = match self.failure.observe_turn(1, 1) {
                        crate::repetition::Verdict::Proceed => None,
                        crate::repetition::Verdict::Warn => {
                            let event = Event::FailureGuard {
                                action: "warned".to_string(),
                            };
                            self.sink.write_event(event.clone())?;
                            self.projector.step(&event);
                            None
                        }
                        crate::repetition::Verdict::Stop => {
                            let event = Event::FailureGuard {
                                action: "stopped".to_string(),
                            };
                            self.sink.write_event(event.clone())?;
                            self.projector.step(&event);
                            Some(StopReason::RepeatedFailure)
                        }
                    };
                    self.commit_turn(turn, 1, 1, failure_stop, snapshot_commit)?;
                    self.fire_post_turn()?;
                    return Ok(failure_stop.map_or(TurnOutcome::Continue, TurnOutcome::Stop));
                }
            }
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
                self.commit_turn(
                    turn,
                    0,
                    0,
                    Some(StopReason::RepetitionGuard),
                    snapshot_commit,
                )?;
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
                self.commit_turn(turn, 0, 0, Some(StopReason::NoProgress), snapshot_commit)?;
                return Ok(TurnOutcome::Stop(StopReason::NoProgress));
            }
        }

        // Last in the chain (ADR-077): the three above are streak-based and all
        // reset on alternation, so an A-B-A-B cycle passes every one of them.
        // This one is windowed, and deliberately the loosest — it only fires
        // once the sharper guards have had their chance.
        match self.oscillation.observe(&actions) {
            crate::repetition::Verdict::Proceed => {}
            crate::repetition::Verdict::Warn => {
                let evt = Event::OscillationGuard {
                    action: "warned".to_string(),
                };
                self.sink.write_event(evt.clone())?;
                self.projector.step(&evt);
            }
            crate::repetition::Verdict::Stop => {
                warn!(
                    guard = "oscillation",
                    "cycling between a few actions; stopping"
                );
                let evt = Event::OscillationGuard {
                    action: "stopped".to_string(),
                };
                self.sink.write_event(evt.clone())?;
                self.projector.step(&evt);
                self.commit_turn(turn, 0, 0, Some(StopReason::Oscillation), snapshot_commit)?;
                return Ok(TurnOutcome::Stop(StopReason::Oscillation));
            }
        }

        let mut terminate_with: Option<(ferric_core::ToolCall, String)> = None;
        let mut plan_terminate_with: Option<String> = None;
        let mut dispatched = 0usize;
        let mut errored = 0usize;
        let mut completion_was_blocked = false;
        for call in &actions {
            if crate::terminator::is_task_complete(&call.name) {
                terminate_with = Some((call.clone(), crate::terminator::summary_of(&call.args)));
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

            if let Some(controller) = self.controller.as_mut() {
                debug!(tool = %call.name, "dispatching evidence-controlled tool call");
                let result = crate::controlled_dispatch::dispatch(
                    turn,
                    call,
                    self.args.registry,
                    self.args.workspace,
                    self.args.provenance,
                    &self.args.sink_policy,
                    self.args.edit_approver,
                    controller,
                    self.sink,
                    &mut self.projector,
                )?;
                debug!(tool = %call.name, is_error = result.is_error, duration_ms = result.duration_ms, "evidence-controlled tool call finished");
                dispatched += 1;
                if result.is_error {
                    errored += 1;
                }
                if let Some(stream_sink) = self.args.stream_sink {
                    let summary_line = result.full.lines().next().unwrap_or("").to_string();
                    let summary = if result.is_error {
                        format!("Error: {summary_line}")
                    } else {
                        summary_line
                    };
                    stream_sink(StreamDelta::ToolCompleted {
                        name: call.name.clone(),
                        summary,
                    });
                }
                continue;
            }

            // Accept-edits gate (ADR-070): a mutating call is previewed to the
            // human, who may reject it before it touches disk. Non-mutating
            // (Read) calls, and runs with no approver, are never gated.
            //
            // This gate and the sink gate inside `Registry::execute` cover the
            // SAME calls — both only ever fire on `Write`/`Execute`, since a
            // tainted `Read` is always allowed. So with accept-edits on, the
            // human was being asked twice about one call (ADR-079). Now they are
            // asked once, here, with the taint disclosed in the preview; an
            // approval here carries through to the sink gate below.
            let permission = self.args.registry.permission_of(&call.name);
            let mutating = matches!(
                permission,
                Some(ferric_guard::PermissionLevel::Write)
                    | Some(ferric_guard::PermissionLevel::Execute)
            );
            let mut human_already_approved = false;
            if let Some(approver) = self.args.edit_approver
                && mutating
            {
                // Disclosure now keys on the RUN, not on a guess about these
                // arguments (ADR-080).
                let untrusted_run = self.args.provenance.is_untrusted();
                if approver(&edit_preview(&call.name, &call.args, untrusted_run)) {
                    human_already_approved = true;
                } else {
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
                human_already_approved,
                self.args.registry,
                self.args.workspace,
                &call.name,
                &call.args,
                self.args.provenance,
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
            if !is_error {
                if call.name == "run_check" {
                    if let Some(name) = call.args.get("name").and_then(|value| value.as_str()) {
                        let evidence = Event::VerificationCheckPassed {
                            turn,
                            name: name.to_string(),
                            mutation_epoch: self.projector.mutation_epoch,
                        };
                        self.sink.write_event(evidence.clone())?;
                        self.projector.step(&evidence);
                    }
                } else if mutating {
                    let mutation_epoch = self.projector.mutation_epoch.saturating_add(1);
                    let mutation = Event::WorkspaceMutation {
                        turn,
                        tool: call.name.clone(),
                        mutation_epoch,
                    };
                    self.sink.write_event(mutation.clone())?;
                    self.projector.step(&mutation);
                }
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

        if let Some((call, summary)) = terminate_with {
            let (required, fresh) = self.completion_evidence();
            let passed = required.len() == fresh.len();
            if !required.is_empty() {
                self.record_completion_gate(required.clone(), fresh.clone(), passed)?;
            }
            if passed {
                self.commit_turn(
                    turn,
                    dispatched,
                    errored,
                    Some(StopReason::TaskComplete),
                    snapshot_commit,
                )?;
                self.projector.last_text = Some(summary);

                self.fire_post_turn()?;

                return Ok(TurnOutcome::Stop(StopReason::TaskComplete));
            }

            let output = crate::projector::completion_gate_message(
                self.projector.mutation_epoch,
                &required,
                &fresh,
            );
            let result = Event::ToolResult {
                id: call.id,
                name: call.name,
                output,
                is_error: true,
                duration_ms: 0,
            };
            self.sink.write_event(result.clone())?;
            self.projector.step(&result);
            dispatched += 1;
            errored += 1;
            completion_was_blocked = true;
        }

        if let Some(plan) = plan_terminate_with {
            self.commit_turn(
                turn,
                dispatched,
                errored,
                Some(StopReason::PlanSubmitted),
                snapshot_commit,
            )?;
            self.projector.last_text = Some(plan);

            self.fire_post_turn()?;

            return Ok(TurnOutcome::Stop(StopReason::PlanSubmitted));
        }

        if dispatched > 0 {
            let (execution_dispatched, execution_errored) =
                crate::failure::execution_counts(dispatched, errored, completion_was_blocked);
            match self
                .failure
                .observe_turn(execution_dispatched, execution_errored)
            {
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
                    self.commit_turn(
                        turn,
                        dispatched,
                        errored,
                        Some(StopReason::RepeatedFailure),
                        snapshot_commit,
                    )?;
                    return Ok(TurnOutcome::Stop(StopReason::RepeatedFailure));
                }
            }
        }
        self.commit_turn(turn, dispatched, errored, None, snapshot_commit)?;
        self.fire_post_turn()?;

        Ok(TurnOutcome::Continue)
    }

    /// Run the configured `post_turn` hook, if any.
    ///
    /// A hook failure is recorded as a trace `Note` and does **not** stop the
    /// loop — unlike `pre_turn`, which does. This body was copy-pasted at all
    /// four turn-exit points; keeping one copy is what stops those four from
    /// drifting apart (ADR-074).
    fn fire_post_turn(&mut self) -> Result<(), FerricError> {
        let Some(hooks) = &self.args.hooks else {
            return Ok(());
        };
        let Some(cmd) = &hooks.post_turn else {
            return Ok(());
        };
        if let Err(e) = crate::hooks_exec::run_hook(cmd, self.args.workspace.root()) {
            let note = Event::Note {
                text: format!("post_turn hook failed: {e}"),
            };
            self.sink.write_event(note.clone())?;
            self.projector.step(&note);
        }
        Ok(())
    }
}

#[allow(clippy::collapsible_if)]
pub async fn run(
    mut args: RunArgs<'_>,
    sink: &mut JsonlSink,
    prompt: Option<&str>,
) -> Result<LoopOutcome, FerricError> {
    if prompt.is_some() && args.answer.is_some() {
        return Err(FerricError::InvalidInput(
            "a resume cannot supply both a generic prompt and --answer".to_string(),
        ));
    }
    match &args.resume {
        Some(replayed) => {
            crate::replay::validate_resume_target(
                replayed,
                args.workspace.root(),
                args.protocol,
                args.harness_policy,
            )
            .map_err(|error| FerricError::InvalidInput(error.to_string()))?;
            match (&replayed.pending_input, args.answer) {
                (Some(_), None) => {
                    return Err(FerricError::InvalidInput(
                        "this continuation is waiting for user input; supply a non-empty answer"
                            .to_string(),
                    ));
                }
                (Some(_), Some(answer)) if answer.trim().is_empty() => {
                    return Err(FerricError::InvalidInput(
                        "the clarification answer must not be empty".to_string(),
                    ));
                }
                (None, Some(_)) => {
                    return Err(FerricError::InvalidInput(
                        "--answer is only valid for a continuation waiting for user input"
                            .to_string(),
                    ));
                }
                _ => {}
            }
        }
        None if args.answer.is_some() => {
            return Err(FerricError::InvalidInput(
                "--answer requires a resumed session".to_string(),
            ));
        }
        None => {}
    }

    // Keep the requested policy optional across every caller boundary. This is
    // the sole resolution point: fresh runs default to Legacy, while resumes
    // inherit their trace unless the operator explicitly selected a match.
    let effective_harness_policy = args
        .harness_policy
        .or(args.resume.as_ref().map(|state| state.harness_policy))
        .unwrap_or_default();
    let controller = match (effective_harness_policy, args.resume.as_ref()) {
        (HarnessPolicy::Legacy, Some(replayed)) | (HarnessPolicy::Evidence, Some(replayed)) => {
            crate::replay::resume_controller_state(replayed, args.registry.required_checks())
                .map_err(|error| FerricError::InvalidInput(error.to_string()))?
        }
        (HarnessPolicy::Legacy, None) => None,
        (HarnessPolicy::Evidence, None) => Some(
            ControllerState::new(
                HarnessPolicy::Evidence,
                args.registry.required_checks().iter().cloned(),
            )
            .map_err(|error| FerricError::InvalidInput(error.to_string()))?,
        ),
        (HarnessPolicy::EvidencePlanner, _) => {
            return Err(FerricError::InvalidInput(
                "harness policy evidence_planner is not implemented yet".to_string(),
            ));
        }
    };
    args.harness_policy = Some(effective_harness_policy);

    // The projector's model-facing cap is not set here: it comes from the
    // `PolicySelected` event below, which carries the registry's value. That
    // is the point of ADR-093 — replay and `trace verify` have only the trace,
    // so the trace is the one source, and run reads it the same way they do.
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
        harness_policy: effective_harness_policy,
        max_turns: u32::from(args.policy.max_turns),
        max_tools: u32::from(args.policy.max_tools),
        prompt_budget_tokens: args.policy.prompt_budget_tokens,
        max_output_tokens: args.policy.max_output_tokens,
        truncation_limit: args.resume.as_ref().map_or_else(
            || args.registry.truncation_limit(),
            |state| state.truncation_limit,
        ),
        tier_source: args.policy.tier_source.label().to_string(),
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

    let registry_tools = registry_tools(
        args.registry,
        args.policy,
        args.protocol,
        effective_harness_policy,
    );

    let turns = match &args.resume {
        Some(replayed) => {
            projector.messages = replayed.messages.clone();
            projector.turns = replayed.turns;
            projector.next_turn = replayed.next_turn;
            projector.last_text = replayed.last_text.clone();
            projector.protocol = Some(replayed.protocol);
            projector.head_len = replayed.head_len;
            projector.committed_turn_starts = replayed.committed_turn_starts.clone();
            projector.guard_history = replayed.guard_history.clone();
            projector.nudged_for_no_action = replayed.nudged_for_no_action;
            projector.truncated_once = replayed.truncated_once;
            projector.last_input_tokens = replayed.last_input_tokens;
            projector.pending_input = replayed.pending_input.clone();
            projector.mutation_epoch = replayed.mutation_epoch;
            projector.passed_checks = replayed.passed_checks.clone();

            let checkpoint = Event::RecoveryCheckpoint {
                state: projector.checkpoint(),
            };
            sink.write_event(checkpoint.clone())?;
            projector.step(&checkpoint);
            if let Some(controller) = controller.as_ref() {
                let checkpoint = Event::ControllerCheckpoint {
                    state: controller.checkpoint(),
                };
                sink.write_event(checkpoint.clone())?;
                projector.step(&checkpoint);
            }

            let clarification = replayed.pending_input.is_some() && args.answer.is_some();
            let amendment =
                if let (Some(request), Some(answer)) = (&replayed.pending_input, args.answer) {
                    Some(format_clarification_answer(request, answer))
                } else {
                    prompt.map(str::to_string)
                };
            if let Some(user) = amendment {
                let resumed = Event::ResumePrompt {
                    user,
                    media: args.media.clone(),
                };
                sink.write_event(resumed.clone())?;
                projector.step(&resumed);
                // The projector atomically consumes pending input when the
                // durable ResumePrompt is applied. The second checkpoint makes
                // that transition self-contained for later resume chains.
                let anchored = Event::RecoveryCheckpoint {
                    state: projector.checkpoint(),
                };
                sink.write_event(anchored.clone())?;
                projector.step(&anchored);
                if let Some(controller) = controller.as_ref() {
                    let anchored = Event::ControllerCheckpoint {
                        state: controller.checkpoint(),
                    };
                    sink.write_event(anchored.clone())?;
                    projector.step(&anchored);
                }
            }
            if !clarification && let Some(controller) = controller.as_ref() {
                let reason = controller
                    .checkpoint()
                    .inherited_pause_reason
                    .ok_or_else(|| {
                        FerricError::InvalidInput(
                            "resumed evidence controller omits its pause reason".to_string(),
                        )
                    })?;
                let packet = controller
                    .recovery_packet(&reason)
                    .map_err(|error| FerricError::Other(error.to_string()))?;
                let message = ControllerState::render_recovery_packet(&packet)
                    .map_err(|error| FerricError::Other(error.to_string()))?;
                let injected = Event::RecoveryPacketInjected { packet, message };
                sink.write_event(injected.clone())?;
                projector.step(&injected);
            }
            replayed.next_turn
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
                for control in crate::terminator::control_descriptors(args.protocol) {
                    system.push_str(&format!("- {}: {}\n", control.name, control.description));
                }
            }
            if effective_harness_policy == HarnessPolicy::Evidence {
                system.push_str("\n\n");
                system.push_str(GENERAL_EVIDENCE_GUIDANCE_V1);
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
            if let Some(controller) = controller.as_ref() {
                let checkpoint = Event::ControllerCheckpoint {
                    state: controller.checkpoint(),
                };
                sink.write_event(checkpoint.clone())?;
                projector.step(&checkpoint);
            }
            0
        }
    };

    let mut offered_names: Vec<String> = registry_tools.iter().map(|t| t.name.clone()).collect();
    let controls = crate::terminator::control_descriptors(args.protocol);
    offered_names.extend(controls.iter().map(|control| control.name.clone()));

    let native_tools: Vec<ToolDescriptor> = {
        let mut v = registry_tools.clone();
        v.extend(controls);
        v
    };

    let mut repetition = crate::repetition::RepetitionGuard::new();
    let mut progress = crate::progress::ProgressGuard::new();
    let mut failure = crate::failure::FailureGuard::new();
    let mut oscillation = crate::oscillation::OscillationGuard::new();
    for guarded in &projector.guard_history {
        let _ = repetition.observe(&guarded.calls);
        let _ = progress.observe(&guarded.calls);
        let _ = oscillation.observe(&guarded.calls);
        // A task_complete proposal with a recorded error result is a blocked
        // completion gate: a passed gate ends successfully and is not
        // resumable. A 0/0 task_complete turn was stopped by an action-shape
        // guard before dispatch, so it must not touch the failure streak.
        // Keep GuardTurn's counts literal and derive the same execution-only
        // view used by the live path.
        let completion_was_blocked = guarded.dispatched > 0
            && guarded.errored > 0
            && guarded
                .calls
                .iter()
                .any(|call| crate::terminator::is_task_complete(&call.name));
        let (execution_dispatched, execution_errored) = crate::failure::execution_counts(
            guarded.dispatched as usize,
            guarded.errored as usize,
            completion_was_blocked,
        );
        if guarded.dispatched > 0 {
            let _ = failure.observe_turn(execution_dispatched, execution_errored);
        }
    }

    let nudged_for_no_action = projector.nudged_for_no_action;
    let truncated_once = projector.truncated_once;
    let last_input_tokens = projector.last_input_tokens;

    let mut state = LoopState {
        args,
        sink,
        projector,
        controller,
        turns,
        turns_this_run: 0,
        offered_names,
        native_tools,
        repetition,
        progress,
        failure,
        oscillation,
        nudged_for_no_action,
        truncated_once,
        last_input_tokens,
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

    if !stop.is_success() {
        let checkpoint = Event::RecoveryCheckpoint {
            state: state.projector.checkpoint(),
        };
        state.sink.write_event(checkpoint.clone())?;
        state.projector.step(&checkpoint);
        if let Some(controller) = state.controller.as_mut() {
            let controller_checkpoint = controller
                .checkpoint_for_pause(stop.as_str())
                .map_err(|error| FerricError::Other(error.to_string()))?;
            let checkpoint = Event::ControllerCheckpoint {
                state: controller_checkpoint.clone(),
            };
            state.sink.write_event(checkpoint.clone())?;
            state.projector.step(&checkpoint);
            *controller = ControllerState::from_checkpoint(&controller_checkpoint)
                .map_err(|error| FerricError::Other(error.to_string()))?;
        }
        let paused = Event::SessionPaused {
            reason: stop.as_str().to_string(),
        };
        state.sink.write_event(paused.clone())?;
        state.projector.step(&paused);
    }

    if stop.is_failure() {
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

    let needs_input = if stop == StopReason::NeedsInput {
        state
            .projector
            .pending_input
            .clone()
            .map(|request| NeedsInput {
                request,
                continuation_id: state.sink.session().to_string(),
            })
    } else {
        None
    };

    Ok(LoopOutcome {
        final_text: (stop != StopReason::NeedsInput)
            .then_some(state.projector.last_text)
            .flatten(),
        stop,
        turns: state.turns,
        needs_input,
    })
}

fn format_clarification_answer(request: &UserInputRequest, answer: &str) -> String {
    format!(
        "[goal amendment: clarification answer]\nQuestion: {}\nContext: {}\nUser answer: {}\nContinue the original objective; this answer amends it and does not replace it.",
        request.question,
        request.context,
        answer.trim()
    )
}

fn registry_tools(
    registry: &Registry,
    policy: &RunPolicy,
    protocol: ActionProtocol,
    harness_policy: HarnessPolicy,
) -> Vec<ToolDescriptor> {
    let specs = match harness_policy {
        HarnessPolicy::Legacy => registry.tools_for_policy(policy),
        HarnessPolicy::Evidence => registry.tools_for_controlled_policy(policy),
        HarnessPolicy::EvidencePlanner => {
            unreachable!("evidence_planner is rejected before tool enumeration")
        }
    };
    specs
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
fn edit_preview(name: &str, args: &serde_json::Value, tainted: bool) -> EditPreview {
    let mut detail = String::new();
    if tainted {
        // The sink policy's question, folded into the one prompt the human
        // already sees, instead of asked separately afterwards (ADR-079).
        detail
            .push_str("WARNING: this run has ingested untrusted research content, so every mutation is gated.\n");
    }
    detail.push_str(&serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string()));
    EditPreview {
        tool: name.to_string(),
        targets: preview_targets(args),
        detail,
    }
}

/// The target path(s) a call names, if any — for showing a human what a call is
/// about to touch.
fn preview_targets(args: &serde_json::Value) -> Vec<String> {
    ["path", "from", "to", "src", "dest"]
        .iter()
        .filter_map(|key| args.get(key).and_then(|v| v.as_str()))
        .map(str::to_string)
        .collect()
}

/// The full, untruncated tool output. It goes to the trace verbatim (ADR-002
/// durability); the model-facing truncation is applied by the projector, which
/// is the single place the context window is assembled.
struct DispatchText {
    full: String,
}

fn dispatch(
    human_already_approved: bool,
    registry: &Registry,
    workspace: &Workspace,
    name: &str,
    args: &serde_json::Value,
    provenance: ferric_guard::Provenance,
    sink_policy: &ferric_guard::SinkPolicy,
) -> (DispatchText, bool, u64, Vec<CheckRecord>) {
    // ADR-074 wired the sink gate to a human; ADR-079 stops it asking twice.
    // When accept-edits already previewed this exact call (taint disclosed) the
    // human has answered, so the sink honours that answer rather than
    // re-prompting. With no accept-edits approver there is nobody to ask, and
    // `RequireApproval` still denies — the safe reading.
    let carry_through = |_r: &ferric_tools::ApprovalRequest<'_>| true;
    let sink_approver: Option<ferric_tools::SinkApprover<'_>> =
        human_already_approved.then_some(&carry_through);

    match registry.execute(
        workspace,
        name,
        args,
        provenance,
        sink_policy,
        sink_approver,
    ) {
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
