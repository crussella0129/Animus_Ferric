use std::time::Duration;

use ferric_core::{ActionProtocol, FerricError, MediaPart, Message, RunPolicy};
use ferric_guard::Workspace;
use ferric_provider::{
    CompletionRequest, Constraint, Provider, SamplingParams, StreamDelta, ToolDescriptor,
};
use ferric_tools::{CheckRecord, ExecuteOutcome, Registry};
use ferric_trace::{Event, JsonlSink};

use crate::outcome::{LoopOutcome, StopReason};

/// Injectable clock for backoff. The default sleeps the calling thread —
/// acceptable because inference backends run their engines on dedicated OS
/// threads; tests inject a recording no-op.
pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration);
}

/// Default `Sleeper`: blocks the current thread.
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// The default system prompt (fallback when no composed prompt is supplied).
/// Deliberately tiny — small contexts are the silent killer (s1 research);
/// per-tier/per-protocol composition via oovra lands in ferric-prompt (s2).
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Ferric, a coding agent. \
Use the available tools to act on the workspace. \
When the task is done, call task_complete with a one-sentence summary. \
Never describe a tool call in prose - actually call the tool.";

/// Prompt-composition genealogy (oovra lineage) for the trace. Plain id+version
/// tuples so ferric-loop needs no ferric-prompt dependency.
pub type PromptLineage = (String, String, Vec<(String, String)>);

/// Everything `run` needs. Borrowed so callers own lifecycle and the loop
/// stays executor-agnostic.
pub struct RunArgs<'a> {
    pub provider: &'a dyn Provider,
    pub registry: &'a Registry,
    pub workspace: &'a Workspace,
    pub policy: &'a RunPolicy,
    pub protocol: ActionProtocol,
    pub sampling: SamplingParams,
    pub sleeper: &'a dyn Sleeper,
    /// Override the built-in system prompt (None = DEFAULT_SYSTEM_PROMPT).
    pub system_prompt: Option<&'a str>,
    /// Composition lineage (output_id, output_version, [(element_id, version)])
    /// — traced as `PromptComposed` when present.
    pub prompt_lineage: Option<PromptLineage>,
    /// Multimodal parts to attach to the first user message (ADR-023). Empty ⇒
    /// the message is text-only, identical to before.
    pub media: Vec<MediaPart>,
    /// Optional live-display sink (ADR-047). `None` (every pre-sprint-37
    /// caller) preserves byte-identical non-streaming behavior — the turn
    /// loop calls the existing `complete_with_backoff`. `Some` drives each
    /// turn's completion via `complete_streaming_with_backoff` instead,
    /// firing `StreamDelta`s to the sink as they become available; the
    /// resulting `Completion` still flows through the exact same
    /// dispatch/validation logic below either way.
    pub stream_sink: Option<&'a (dyn Fn(StreamDelta) + Sync)>,
    /// Resume an interrupted, still-incomplete session (sprint 39, ADR-049).
    /// `None` (every pre-sprint-39 caller) is byte-identical to today: a
    /// fresh `[system, user]` history, `SessionPrompt` written, `resumed_from`
    /// `None`. `Some` seeds `messages`/`turns`/`last_text` from it instead,
    /// writes `resumed_from`, and skips `SessionPrompt` (there's no new
    /// initial prompt for this session — its own prompt lives in the session
    /// it resumed from).
    pub resume: Option<crate::replay::ReplayedState>,
}

use crate::projector::TraceProjector;

/// Run the agent loop for one user prompt. `prompt` is `Option` to support a
/// pure continuation of a resumed session with no new instruction — required
/// when `resume` is `None` (the CLI layer guarantees this; see `resume` on
/// `RunArgs`), and optional (an extra nudge appended after the replayed
/// history) when resuming. Trace I/O errors abort with `Err`; everything else
/// (provider failures included) folds into the outcome.
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

    let mut turns = match &args.resume {
        Some(replayed) => {
            // A resumed session is already hydrated; we just seed the projector
            // so we don't have to replay the trace lines from disk again.
            projector.messages = replayed.messages.clone();
            projector.turns = replayed.turns;
            projector.last_text = replayed.last_text.clone();
            projector.protocol = Some(replayed.protocol);
            projector.head_len = replayed.messages.len(); // A deliberate oversimplification: history folds will only apply to new turns.
            // Note: we'd need more state here if resuming *mid-turn* or supporting history compactor resuming properly,
            // but for now, we just restore what ReplayedState provides.
            replayed.turns
        }
        None => {
            let system = args.system_prompt.unwrap_or(DEFAULT_SYSTEM_PROMPT);
            let prompt_text = prompt.ok_or_else(|| {
                FerricError::InvalidInput(
                    "run() requires a prompt when not resuming a session".to_string(),
                )
            })?;
            let session_prompt = Event::SessionPrompt {
                system: system.to_string(),
                user: prompt_text.to_string(),
                media: args.media.clone(),
            };
            sink.write_event(session_prompt.clone())?;
            projector.step(&session_prompt);
            0
        }
    };

    // A resumed session MAY also carry one new user-supplied nudge, appended
    // after the replayed history (T-3905's CLI layer decides when this is
    // populated; still confined to a still-incomplete task, never a fresh
    // instruction on an already-finished one — `replay()` already refuses
    // those).
    if args.resume.is_some()
        && let Some(extra) = prompt
    {
        projector.messages.push(Message::user(extra));
    }

    // Registry tools (no terminator) drive both the native tools list and the
    // grammar schema; the terminator is appended where each mode needs it.
    let registry_tools = registry_tools(args.registry, args.policy);
    let mut offered_names: Vec<String> = registry_tools.iter().map(|t| t.name.clone()).collect();
    offered_names.push(crate::terminator::TASK_COMPLETE.to_string());

    let native_tools: Vec<ToolDescriptor> = {
        let mut v = registry_tools.clone();
        v.push(crate::terminator::descriptor());
        v
    };

    let mut repetition = crate::repetition::RepetitionGuard::new();
    let mut progress = crate::progress::ProgressGuard::new();
    let mut failure = crate::failure::FailureGuard::new();
    let mut nudged_for_no_action = false;
    let mut truncated_once = false;

    // Context-budget compaction (sprint 40, ADR-050). `head_len` covers the
    // entire seeded history (fresh OR resumed) — on a resumed session only
    // NEW turns generated after resuming are foldable, a deliberate v1 scope
    // limit. `last_input_tokens` is the last real signal a completion
    // reported; compaction is checked against it at the top of the NEXT
    // iteration, before that turn's request is assembled.
    let mut last_input_tokens: Option<u32> = None;

    let stop = 'outer: loop {
        if turns >= u32::from(args.policy.max_turns) {
            break StopReason::MaxTurns;
        }
        let turn = turns;
        turns += 1;
        let start_event = Event::TurnStart { turn };
        sink.write_event(start_event.clone())?;
        projector.step(&start_event);

        if let Some(event) = crate::compact::maybe_compact(
            &projector,
            args.provider,
            args.sleeper,
            args.policy,
            last_input_tokens,
        ).await? {
            sink.write_event(event.clone())?;
            projector.step(&event);
        }

        // Build the request per protocol (ADR-010/ADR-015: constraint and
        // tools are mutually exclusive by construction — the invalid state is
        // unrepresentable here).
        let tools = match args.protocol {
            ActionProtocol::NativeTools => native_tools.clone(),
            ActionProtocol::ConstrainedJson | ActionProtocol::TextXml => Vec::new(),
        };
        // ConstrainedJson carries the harness-authored action schema as a real
        // server-enforced constraint (the thesis). TextXml stays unconstrained
        // (the model emits XML; the loop scrapes it). Native uses tools.
        let constraint = match args.protocol {
            ActionProtocol::ConstrainedJson => Some(Constraint::JsonSchema(
                crate::grammar::action_schema(&registry_tools),
            )),
            ActionProtocol::NativeTools | ActionProtocol::TextXml => None,
        };
        let request = CompletionRequest {
            messages: projector.messages.clone(),
            sampling: args.sampling.clone(),
            tools,
            constraint,
        };
        if let Err(e) = request.validate() {
            let note = Event::Note {
                text: format!("invalid request constructed by loop: {e}"),
            };
            sink.write_event(note.clone())?;
            projector.step(&note);
            break StopReason::ProviderError;
        }
        
        let assembled = Event::PromptAssembled {
            turn,
            message_count: projector.messages.len() as u32,
            chars: projector.messages
                .iter()
                .map(|m| m.text.as_deref().unwrap_or_default().len() as u64)
                .sum(),
            offered_tools: offered_names.clone(),
        };
        sink.write_event(assembled.clone())?;
        projector.step(&assembled);

        if args.protocol == ActionProtocol::ConstrainedJson {
            let evt = Event::ConstraintApplied {
                kind: "json_schema".to_string(),
            };
            sink.write_event(evt.clone())?;
            projector.step(&evt);
        }

        let completion_result = match args.stream_sink {
            Some(on_delta) => {
                crate::backoff::complete_streaming_with_backoff(
                    args.provider,
                    request,
                    args.sleeper,
                    on_delta,
                )
                .await
            }
            None => {
                crate::backoff::complete_with_backoff(args.provider, request, args.sleeper).await
            }
        };
        let completion = match completion_result {
            Ok(completion) => completion,
            Err(e) => {
                sink.write_event(Event::Note {
                    text: format!("provider error: {e}"),
                })?;
                break StopReason::ProviderError;
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
        sink.write_event(turn_end.clone())?;
        projector.step(&turn_end);
        
        last_input_tokens = completion.input_tokens;

        if args.protocol == ActionProtocol::ConstrainedJson && completion.truncated {
            if truncated_once {
                break StopReason::TruncatedAction;
            }
            truncated_once = true;
            // The truncation message is generated dynamically by Projector logic, 
            // but we need to commit this turn immediately so it shows up in messages
            // for the retry. Wait, `Projector::commit_pending()` will flush it!
            // Instead of doing it manually, `continue` will hit the NEXT `TurnStart`
            // and commit it. BUT `run.rs` checks tokens and builds request.
            continue;
        }

        // Best-effort final text is native-mode only: in grammar mode the
        // assistant text IS the action JSON, never a final answer.
        // Handled completely by `TraceProjector` during commit.

        let (actions, _parse_error) = match args.protocol {
            ActionProtocol::NativeTools => (completion.message.tool_calls.clone(), None),
            ActionProtocol::ConstrainedJson => {
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
            let is_native_final = args.protocol == ActionProtocol::NativeTools
                && completion
                    .message
                    .text
                    .as_deref()
                    .is_some_and(|t| !t.trim().is_empty());
            if is_native_final {
                break StopReason::FinalText;
            }
            if nudged_for_no_action {
                break StopReason::EmptyCompletion;
            }
            nudged_for_no_action = true;
            
            // `TraceProjector` builds the nudge upon commit. We just continue.
            continue;
        }

        // Repetition guard (hash ALL actions in the turn).
        match repetition.observe(&actions) {
            crate::repetition::Verdict::Proceed => {}
            crate::repetition::Verdict::Warn => {
                let evt = Event::RepetitionGuard {
                    action: "warned".to_string(),
                };
                sink.write_event(evt.clone())?;
                projector.step(&evt);
            }
            crate::repetition::Verdict::Stop => {
                let evt = Event::RepetitionGuard {
                    action: "stopped".to_string(),
                };
                sink.write_event(evt.clone())?;
                projector.step(&evt);
                break StopReason::RepetitionGuard;
            }
        }

        // No-progress guard (same tool NAME repeated with different args — the
        // "semantic flailing" mode the repetition guard misses, ADR-031/037).
        // Looser threshold than repetition; bounds wasted compute on a stuck
        // model and emits a precise `no_progress` reason instead of max_turns.
        match progress.observe(&actions) {
            crate::repetition::Verdict::Proceed => {}
            crate::repetition::Verdict::Warn => {
                let evt = Event::NoProgressGuard {
                    action: "warned".to_string(),
                };
                sink.write_event(evt.clone())?;
                projector.step(&evt);
            }
            crate::repetition::Verdict::Stop => {
                let evt = Event::NoProgressGuard {
                    action: "stopped".to_string(),
                };
                sink.write_event(evt.clone())?;
                projector.step(&evt);
                break StopReason::NoProgress;
            }
        }

        // Dispatch actions in order; intercept the terminator. Tally the turn's
        // dispatched (non-terminator) calls + how many errored, for the
        // repeated-failure guard below.
        let mut terminate_with: Option<String> = None;
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
                sink.write_event(tc.clone())?;
                projector.step(&tc);
                continue;
            }
            let tc = Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            };
            sink.write_event(tc.clone())?;
            projector.step(&tc);
            
            let (result_text, is_error, duration_ms, checks) =
                dispatch(args.registry, args.workspace, &call.name, &call.args);
            dispatched += 1;
            if is_error {
                errored += 1;
            }
            for check in &checks {
                let evt = permission_event(check);
                sink.write_event(evt.clone())?;
                projector.step(&evt);
            }
            let tr = Event::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                output: result_text.full.clone(),
                is_error,
                duration_ms,
            };
            sink.write_event(tr.clone())?;
            projector.step(&tr);
        }
        if let Some(summary) = terminate_with {
            // Need to commit pending so last_text gets evaluated for native mode.
            projector.commit_pending();
            // The terminator's summary string is the definitive final text for ALL protocols.
            projector.last_text = Some(summary);
            break 'outer StopReason::TaskComplete;
        }

        // Repeated-failure guard (keys off RESULTS — runs after dispatch, only
        // on a non-terminating turn). Catches the "different tools, all failing"
        // mode the repetition + no-progress guards reset on (ADR-038).
        if dispatched > 0 {
            match failure.observe_turn(dispatched, errored) {
                crate::repetition::Verdict::Proceed => {}
                crate::repetition::Verdict::Warn => {
                    let evt = Event::FailureGuard {
                        action: "warned".to_string(),
                    };
                    sink.write_event(evt.clone())?;
                    projector.step(&evt);
                }
                crate::repetition::Verdict::Stop => {
                    let evt = Event::FailureGuard {
                        action: "stopped".to_string(),
                    };
                    sink.write_event(evt.clone())?;
                    projector.step(&evt);
                    break StopReason::RepeatedFailure;
                }
            }
        }
    };

    let session_end = Event::SessionEnd {
        reason: stop.as_str().to_string(),
    };
    sink.write_event(session_end.clone())?;
    projector.step(&session_end);
    
    // Explicitly commit pending to gather the final last_text, etc.
    projector.commit_pending();

    Ok(LoopOutcome {
        final_text: projector.last_text,
        stop,
        turns,
    })
}


/// The policy-filtered registry tool descriptors (terminator NOT included).
fn registry_tools(registry: &Registry, policy: &RunPolicy) -> Vec<ToolDescriptor> {
    registry
        .tools_for_policy(policy)
        .into_iter()
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
) -> (DispatchText, bool, u64, Vec<CheckRecord>) {
    match registry.execute(workspace, name, args) {
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
