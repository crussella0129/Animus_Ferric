use std::time::Duration;

use ferric_core::{
    ActionProtocol, FerricError, HarnessPolicy, MediaPart, RunPolicy, ToolCall, UserInputRequest,
};
use ferric_guard::Workspace;
use ferric_provider::{
    CompletionRequest, Constraint, Provider, SamplingParams, StreamDelta, ToolDescriptor,
};
use ferric_tools::{ExecuteOutcome, Registry};
use ferric_trace::{Event, JsonlSink, RECOVERY_CHECKPOINT_VERSION, RecoveryCheckpointV1};
use tracing::{debug, info, instrument, warn};

use crate::ControllerState;
use crate::controlled_dispatch::{
    DispatchResult, EDIT_REJECTED, edit_preview, record_permission_checks, record_result,
    sink_approver,
};
use crate::outcome::{LoopOutcome, NeedsInput, StopReason};
use crate::projector::{TraceProjector, record_event};
use crate::repetition::Verdict;

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

const GENERAL_EVIDENCE_GUIDANCE_V2: &str = "[Ferric general evidence guidance v2]\n\
Before the first mutation, completely inspect every existing task-scoped workspace file explicitly named by the task and permitted for modification; paginate incomplete reads until coverage is complete.\n\
Genuinely absent paths may be created directly without a prior file read.\n\
After a failed authorized check, inspect and repair the implementation implicated by its diagnostic in a later turn; do not change unrelated tests or files.\n\
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
    fn record(&mut self, event: Event) -> Result<(), FerricError> {
        record_event(event, self.sink, &mut self.projector)
    }

    /// Commit `turn` at its durable barrier; the stop (if any) decides the
    /// turn's outcome.
    fn commit_turn(
        &mut self,
        turn: u32,
        dispatched: usize,
        errored: usize,
        stop: Option<StopReason>,
        snapshot_commit: Option<String>,
    ) -> Result<TurnOutcome, FerricError> {
        self.record(Event::TurnCommitted {
            turn,
            dispatched: dispatched as u32,
            errored: errored as u32,
            stop_reason: stop.map(|reason| reason.as_str().to_string()),
            snapshot_commit,
        })?;
        Ok(stop.map_or(TurnOutcome::Continue, TurnOutcome::Stop))
    }

    fn record_call(&mut self, call: &ToolCall) -> Result<(), FerricError> {
        self.record(Event::ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.args.clone(),
        })
    }

    fn record_synthetic_result(
        &mut self,
        call: &ToolCall,
        output: String,
        is_error: bool,
    ) -> Result<(), FerricError> {
        self.record_call(call)?;
        record_result(call, output, is_error, 0, self.sink, &mut self.projector)?;
        Ok(())
    }

    /// Record a guard's verdict as its `warned`/`stopped` trace event.
    /// Returns whether the guard stopped the loop.
    fn record_guard(
        &mut self,
        verdict: Verdict,
        event: fn(String) -> Event,
    ) -> Result<bool, FerricError> {
        let action = match verdict {
            Verdict::Proceed => return Ok(false),
            Verdict::Warn => "warned",
            Verdict::Stop => "stopped",
        };
        self.record(event(action.to_string()))?;
        Ok(verdict == Verdict::Stop)
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
        self.record(Event::CompletionGate {
            mutation_epoch,
            required_checks,
            fresh_checks,
            decision: if passed { "passed" } else { "blocked" }.to_string(),
        })
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
                    self.record(Event::Note {
                        text: format!("pre_turn hook failed: {e}"),
                    })?;
                    return Ok(TurnOutcome::Stop(StopReason::HookFailed));
                }
            }
        }

        let turn = self.turns;
        self.turns += 1;
        self.turns_this_run += 1;
        self.record(Event::TurnStart { turn })?;

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
            self.record(event)?;
        }

        let tools = match self.args.protocol {
            ActionProtocol::NativeTools => self.native_tools.clone(),
            ActionProtocol::ConstrainedJson | ActionProtocol::TextXml | ActionProtocol::Plan => {
                Vec::new()
            }
        };

        let constraint = match self.args.protocol {
            ActionProtocol::ConstrainedJson | ActionProtocol::Plan => Some(Constraint::JsonSchema(
                ferric_iron::action_schema(&self.native_tools),
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
            self.record(Event::Note {
                text: format!("invalid request constructed by loop: {e}"),
            })?;
            return Ok(TurnOutcome::Stop(StopReason::ProviderError));
        }

        self.record(Event::PromptAssembled {
            turn,
            message_count: self.projector.messages.len() as u32,
            chars: self
                .projector
                .messages
                .iter()
                .map(|m| m.text.as_deref().unwrap_or_default().len() as u64)
                .sum(),
            offered_tools: self.offered_names.clone(),
        })?;

        if self.args.protocol == ActionProtocol::ConstrainedJson
            || self.args.protocol == ActionProtocol::Plan
        {
            self.record(Event::ConstraintApplied {
                kind: "json_schema".to_string(),
            })?;
        }

        let budget = self
            .args
            .policy
            .output_budget
            .as_ref()
            .filter(|budget| {
                budget.effective == request.sampling.max_tokens
                    && self.args.policy.max_output_tokens == request.sampling.max_tokens
            })
            .cloned()
            .unwrap_or_else(|| ferric_core::OutputBudget {
                requested: None,
                effective: request.sampling.max_tokens,
                declared_ctx: self
                    .args
                    .policy
                    .output_budget
                    .as_ref()
                    .and_then(|budget| budget.declared_ctx),
                source: if request.sampling.max_tokens == self.args.policy.max_output_tokens {
                    ferric_core::OutputBudgetSource::Policy
                } else {
                    ferric_core::OutputBudgetSource::Caller
                },
            });
        self.sink
            .write_event(Event::MainActionBudget { turn, budget })?;

        let completion = match crate::backoff::complete_with_backoff(
            self.args.provider,
            request,
            self.args.sleeper,
            self.args.stream_sink,
            self.args.cancel_flag.clone(),
        )
        .await
        {
            Ok(completion) => completion,
            Err(e) => {
                warn!(error = %e, "provider error after retries; stopping");
                self.sink.write_event(Event::Note {
                    text: format!("provider error: {e}"),
                })?;
                return Ok(TurnOutcome::Stop(StopReason::ProviderError));
            }
        };

        self.record(Event::TurnEnd {
            turn,
            text: completion.message.text.clone(),
            tool_call_count: completion.message.tool_calls.len() as u32,
            input_tokens: completion.input_tokens,
            output_tokens: completion.output_tokens,
            truncated: completion.truncated,
        })?;

        let vcs = crate::vcs::Vcs::new(self.args.workspace.root());
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
            self.record(Event::ActionsProposed {
                turn,
                calls: Vec::new(),
            })?;
            let stop = self.truncated_once.then_some(StopReason::TruncatedAction);
            self.truncated_once = true;
            return self.commit_turn(turn, 0, 0, stop, snapshot_commit);
        }

        let (actions, parse_error) = match self.args.protocol {
            ActionProtocol::NativeTools => (completion.message.tool_calls.clone(), None),
            ActionProtocol::ConstrainedJson | ActionProtocol::Plan => {
                match ferric_iron::parse_json_action(
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

        self.record(Event::ActionsProposed {
            turn,
            calls: actions.clone(),
        })?;

        if actions.is_empty() {
            // Record WHY there was no action. Without this a grammar failure and
            // a genuinely empty completion are indistinguishable in the trace,
            // which makes post-hoc analysis of small-model behaviour guesswork.
            if let Some(e) = &parse_error {
                debug!(error = %e, "action parse failed");
                self.record(Event::Note {
                    text: format!("action parse failed: {e}"),
                })?;
            }

            let is_native_final = self.args.protocol == ActionProtocol::NativeTools
                && completion
                    .message
                    .text
                    .as_deref()
                    .is_some_and(|t| !t.trim().is_empty());
            if is_native_final {
                let (required, fresh) = self.completion_evidence();
                let passed = required.len() == fresh.len();
                if !required.is_empty() {
                    self.record_completion_gate(required, fresh, passed)?;
                }
                let stop = passed.then_some(StopReason::FinalText);
                let outcome = self.commit_turn(turn, 0, 0, stop, snapshot_commit)?;
                self.fire_post_turn()?;
                return Ok(outcome);
            }
            let stop = self
                .nudged_for_no_action
                .then_some(StopReason::EmptyCompletion);
            self.nudged_for_no_action = true;
            return self.commit_turn(turn, 0, 0, stop, snapshot_commit);
        }

        // Clarification is a control transition, not an executable tool. It
        // must be intercepted before any action-shape guard so the act of
        // declaring ambiguity cannot be mistaken for repetition or flailing.
        if actions
            .iter()
            .any(|call| ferric_iron::terminator::is_request_user_input(&call.name))
        {
            let rejection = match actions.as_slice() {
                [call] => match ferric_iron::terminator::request_of(&call.args) {
                    Ok(request) => {
                        self.record_synthetic_result(
                            call,
                            "user input requested; session paused until an answer is supplied"
                                .to_string(),
                            false,
                        )?;
                        self.projector.pending_input = Some(request);
                        let stop = Some(StopReason::NeedsInput);
                        let outcome = self.commit_turn(turn, 1, 0, stop, snapshot_commit)?;
                        self.fire_post_turn()?;
                        return Ok(outcome);
                    }
                    Err(error) => error.to_string(),
                },
                _ => {
                    "request_user_input must be the only action in its turn; no calls were executed"
                        .to_string()
                }
            };
            for call in &actions {
                self.record_synthetic_result(call, rejection.clone(), true)?;
            }
            let failed = actions.len();
            let verdict = self.failure.observe_turn(failed, failed);
            let stop = self
                .record_guard(verdict, |action| Event::FailureGuard { action })?
                .then_some(StopReason::RepeatedFailure);
            let outcome = self.commit_turn(turn, failed, failed, stop, snapshot_commit)?;
            self.fire_post_turn()?;
            return Ok(outcome);
        }

        let verdict = self.repetition.observe(&actions);
        if self.record_guard(verdict, |action| Event::RepetitionGuard { action })? {
            warn!(guard = "repetition", "identical action repeated; stopping");
            let stop = Some(StopReason::RepetitionGuard);
            return self.commit_turn(turn, 0, 0, stop, snapshot_commit);
        }

        let verdict = self.progress.observe(&actions);
        if self.record_guard(verdict, |action| Event::NoProgressGuard { action })? {
            warn!(
                guard = "no_progress",
                "same tool with churning args; stopping"
            );
            let stop = Some(StopReason::NoProgress);
            return self.commit_turn(turn, 0, 0, stop, snapshot_commit);
        }

        // Last in the chain (ADR-077): the three above are streak-based and all
        // reset on alternation, so an A-B-A-B cycle passes every one of them.
        // This one is windowed, and deliberately the loosest — it only fires
        // once the sharper guards have had their chance.
        let verdict = self.oscillation.observe(&actions);
        if self.record_guard(verdict, |action| Event::OscillationGuard { action })? {
            warn!(
                guard = "oscillation",
                "cycling between a few actions; stopping"
            );
            let stop = Some(StopReason::Oscillation);
            return self.commit_turn(turn, 0, 0, stop, snapshot_commit);
        }

        let mut terminate_with: Option<(&ToolCall, String)> = None;
        let mut plan_terminate_with: Option<String> = None;
        let mut dispatched = 0usize;
        let mut errored = 0usize;
        let mut controller_blocks = 0usize;
        let mut completion_was_blocked = false;
        for call in &actions {
            self.record_call(call)?;
            if ferric_iron::terminator::is_task_complete(&call.name) {
                terminate_with = Some((call, ferric_iron::terminator::summary_of(&call.args)));
                continue;
            }
            if ferric_iron::terminator::is_submit_plan(&call.name) {
                plan_terminate_with = Some(ferric_iron::terminator::plan_of(&call.args));
                continue;
            }

            let result = match self.controller.as_mut() {
                Some(controller) => {
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
                    result
                }
                None => match self.legacy_dispatch(turn, call)? {
                    Some(result) => result,
                    // Rejected in accept-edits: recorded, but never streamed.
                    None => {
                        dispatched += 1;
                        errored += 1;
                        continue;
                    }
                },
            };
            dispatched += 1;
            if result.is_error {
                errored += 1;
            }
            if result.controller_blocked {
                controller_blocks += 1;
            }
            if let Some(stream_sink) = self.args.stream_sink {
                // Determine a short summary (first line) for the stream display.
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
        }

        if let Some((call, summary)) = terminate_with {
            let (required, fresh) = self.completion_evidence();
            let passed = required.len() == fresh.len();
            if !required.is_empty() {
                self.record_completion_gate(required.clone(), fresh.clone(), passed)?;
            }
            if passed {
                let stop = Some(StopReason::TaskComplete);
                let outcome = self.commit_turn(turn, dispatched, errored, stop, snapshot_commit)?;
                self.projector.last_text = Some(summary);
                self.fire_post_turn()?;
                return Ok(outcome);
            }

            let output = crate::projector::completion_gate_message(
                self.projector.mutation_epoch,
                &required,
                &fresh,
            );
            record_result(call, output, true, 0, self.sink, &mut self.projector)?;
            dispatched += 1;
            errored += 1;
            completion_was_blocked = true;
        }

        if let Some(plan) = plan_terminate_with {
            let stop = Some(StopReason::PlanSubmitted);
            let outcome = self.commit_turn(turn, dispatched, errored, stop, snapshot_commit)?;
            self.projector.last_text = Some(plan);
            self.fire_post_turn()?;
            return Ok(outcome);
        }

        if let Some((execution_dispatched, execution_errored)) = crate::failure::failure_observation(
            dispatched,
            errored,
            controller_blocks,
            completion_was_blocked,
        ) {
            let verdict = self
                .failure
                .observe_turn(execution_dispatched, execution_errored);
            if self.record_guard(verdict, |action| Event::FailureGuard { action })? {
                warn!(
                    guard = "failure",
                    dispatched, errored, "every tool call erroring; stopping"
                );
                let stop = Some(StopReason::RepeatedFailure);
                return self.commit_turn(turn, dispatched, errored, stop, snapshot_commit);
            }
        }
        let outcome = self.commit_turn(turn, dispatched, errored, None, snapshot_commit)?;
        self.fire_post_turn()?;
        Ok(outcome)
    }

    /// Legacy-policy dispatch through `Registry::execute`. `None` means the
    /// human rejected the call in accept-edits mode; that refusal is already
    /// recorded as the call's result.
    fn legacy_dispatch(
        &mut self,
        turn: u32,
        call: &ToolCall,
    ) -> Result<Option<DispatchResult>, FerricError> {
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
        let mutating = matches!(
            self.args.registry.permission_of(&call.name),
            Some(ferric_guard::PermissionLevel::Write)
                | Some(ferric_guard::PermissionLevel::Execute)
        );
        let mut human_already_approved = false;
        if let Some(approver) = self.args.edit_approver
            && mutating
        {
            // Disclosure now keys on the RUN, not on a guess about these
            // arguments (ADR-080).
            if approver(&edit_preview(call, self.args.provenance.is_untrusted())) {
                human_already_approved = true;
            } else {
                warn!(tool = %call.name, "edit rejected by user (accept-edits)");
                let rejected = EDIT_REJECTED.to_string();
                record_result(call, rejected, true, 0, self.sink, &mut self.projector)?;
                return Ok(None);
            }
        }

        debug!(tool = %call.name, "dispatching tool call");
        let (full, is_error, duration_ms, checks) = match self.args.registry.execute(
            self.args.workspace,
            &call.name,
            &call.args,
            self.args.provenance,
            &self.args.sink_policy,
            sink_approver(human_already_approved),
        ) {
            ExecuteOutcome::Completed {
                output,
                duration_ms,
                checks,
            } => (output.full, output.is_error, duration_ms, checks),
            ExecuteOutcome::Denied { reason, checks } => {
                (format!("DENIED: {reason}"), true, 0, checks)
            }
            ExecuteOutcome::UnknownTool { name } => {
                (format!("unknown tool: {name}"), true, 0, Vec::new())
            }
        };
        debug!(tool = %call.name, is_error, duration_ms, "tool call finished");
        record_permission_checks(&checks, self.sink, &mut self.projector)?;
        if !is_error {
            if call.name == "run_check" {
                if let Some(name) = call.args.get("name").and_then(|value| value.as_str()) {
                    self.record(Event::VerificationCheckPassed {
                        turn,
                        name: name.to_string(),
                        mutation_epoch: self.projector.mutation_epoch,
                    })?;
                }
            } else if mutating {
                self.record(Event::WorkspaceMutation {
                    turn,
                    tool: call.name.clone(),
                    mutation_epoch: self.projector.mutation_epoch.saturating_add(1),
                })?;
            }
        }
        // The full, untruncated output goes to the trace verbatim (ADR-002
        // durability); the projector applies the model-facing truncation.
        record_result(
            call,
            full,
            is_error,
            duration_ms,
            self.sink,
            &mut self.projector,
        )
        .map(Some)
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
            self.record(Event::Note {
                text: format!("post_turn hook failed: {e}"),
            })?;
        }
        Ok(())
    }
}

/// Anchor the controller truth (Evidence only) that must immediately follow a
/// session's state base or recovery checkpoint.
fn record_controller_checkpoint(
    controller: Option<&ControllerState>,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<(), FerricError> {
    match controller {
        Some(controller) => record_event(
            Event::ControllerCheckpoint {
                state: controller.checkpoint(),
            },
            sink,
            projector,
        ),
        None => Ok(()),
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
            match (&replayed.core.pending_input, args.answer) {
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

    record_event(
        Event::SessionStart {
            workspace: args.workspace.root().display().to_string(),
            resumed_from: args.resume.as_ref().map(|r| r.source_session.clone()),
        },
        sink,
        &mut projector,
    )?;

    record_event(
        Event::PolicySelected {
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
        },
        sink,
        &mut projector,
    )?;

    if let Some((output_id, output_version, composed_of)) = &args.prompt_lineage {
        record_event(
            Event::PromptComposed {
                output_id: output_id.clone(),
                output_version: output_version.clone(),
                composed_of: composed_of.clone(),
            },
            sink,
            &mut projector,
        )?;
    }

    let registry_tools = registry_tools(
        args.registry,
        args.policy,
        args.protocol,
        effective_harness_policy,
    );

    let turns = match &args.resume {
        Some(replayed) => {
            // The inherited core state, re-stamped at the current version, is
            // this trace's self-contained base; projecting it seeds the
            // projector exactly as replay left it.
            let base = RecoveryCheckpointV1 {
                version: RECOVERY_CHECKPOINT_VERSION,
                ..replayed.core.clone()
            };
            record_event(
                Event::RecoveryCheckpoint { state: base },
                sink,
                &mut projector,
            )?;
            record_controller_checkpoint(controller.as_ref(), sink, &mut projector)?;

            let pending_input = &replayed.core.pending_input;
            let clarification = pending_input.is_some() && args.answer.is_some();
            let amendment = if let (Some(request), Some(answer)) = (pending_input, args.answer) {
                Some(format_clarification_answer(request, answer))
            } else {
                prompt.map(str::to_string)
            };
            if let Some(user) = amendment {
                let resumed = Event::ResumePrompt {
                    user,
                    media: args.media.clone(),
                };
                record_event(resumed, sink, &mut projector)?;
                // The projector atomically consumes pending input when the
                // durable ResumePrompt is applied. The second checkpoint makes
                // that transition self-contained for later resume chains.
                let anchored = Event::RecoveryCheckpoint {
                    state: projector.checkpoint(),
                };
                record_event(anchored, sink, &mut projector)?;
                record_controller_checkpoint(controller.as_ref(), sink, &mut projector)?;
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
                record_event(injected, sink, &mut projector)?;
            }
            replayed.core.next_turn
        }
        None => {
            let mut system = args
                .system_prompt
                .unwrap_or(DEFAULT_SYSTEM_PROMPT)
                .to_string();
            if args.system_prompt.is_none() {
                let listed: Vec<ToolDescriptor> = registry_tools
                    .iter()
                    .cloned()
                    .chain(ferric_iron::terminator::control_descriptors(args.protocol))
                    .collect();
                system.push_str(&ferric_iron::render_tool_listing(&listed));
            }
            if effective_harness_policy == HarnessPolicy::Evidence {
                system.push_str("\n\n");
                system.push_str(GENERAL_EVIDENCE_GUIDANCE_V2);
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
            record_event(session_prompt, sink, &mut projector)?;
            record_controller_checkpoint(controller.as_ref(), sink, &mut projector)?;
            0
        }
    };

    let mut offered_names: Vec<String> = registry_tools.iter().map(|t| t.name.clone()).collect();
    let controls = ferric_iron::terminator::control_descriptors(args.protocol);
    offered_names.extend(controls.iter().map(|control| control.name.clone()));

    let native_tools: Vec<ToolDescriptor> = {
        let mut v = registry_tools.clone();
        v.extend(controls);
        v
    };

    let mut repetition = crate::repetition::RepetitionGuard::default();
    let mut progress = crate::progress::ProgressGuard::default();
    let mut failure = crate::failure::FailureGuard::default();
    let mut oscillation = crate::oscillation::OscillationGuard::default();
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
                .any(|call| ferric_iron::terminator::is_task_complete(&call.name));
        if let Some((execution_dispatched, execution_errored)) = crate::failure::failure_observation(
            guarded.dispatched as usize,
            guarded.errored as usize,
            guarded.controller_blocks as usize,
            completion_was_blocked,
        ) {
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

    state.record(Event::SessionEnd {
        reason: stop.as_str().to_string(),
    })?;

    if !stop.is_success() {
        state.record(Event::RecoveryCheckpoint {
            state: state.projector.checkpoint(),
        })?;
        if let Some(controller) = state.controller.as_mut() {
            let controller_checkpoint = controller
                .checkpoint_for_pause(stop.as_str())
                .map_err(|error| FerricError::Other(error.to_string()))?;
            let checkpoint = Event::ControllerCheckpoint {
                state: controller_checkpoint.clone(),
            };
            record_event(checkpoint, state.sink, &mut state.projector)?;
            *controller = ControllerState::from_checkpoint(&controller_checkpoint)
                .map_err(|error| FerricError::Other(error.to_string()))?;
        }
        state.record(Event::SessionPaused {
            reason: stop.as_str().to_string(),
        })?;
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
        .map(ToolDescriptor::from)
        .collect()
}
