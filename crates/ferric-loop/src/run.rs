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
    sink.write_event(Event::SessionStart {
        workspace: args.workspace.root().display().to_string(),
        resumed_from: args.resume.as_ref().map(|r| r.source_session.clone()),
    })?;
    sink.write_event(Event::PolicySelected {
        tier: args.policy.tier,
        protocol: args.protocol,
        max_turns: u32::from(args.policy.max_turns),
        max_tools: u32::from(args.policy.max_tools),
        prompt_budget_tokens: args.policy.prompt_budget_tokens,
        max_output_tokens: args.policy.max_output_tokens,
    })?;
    if let Some((output_id, output_version, composed_of)) = &args.prompt_lineage {
        sink.write_event(Event::PromptComposed {
            output_id: output_id.clone(),
            output_version: output_version.clone(),
            composed_of: composed_of.clone(),
        })?;
    }

    let (mut messages, mut turns, mut last_text) = match &args.resume {
        Some(replayed) => (
            replayed.messages.clone(),
            replayed.turns,
            replayed.last_text.clone(),
        ),
        None => {
            let system = args.system_prompt.unwrap_or(DEFAULT_SYSTEM_PROMPT);
            let prompt = prompt.ok_or_else(|| {
                FerricError::InvalidInput(
                    "run() requires a prompt when not resuming a session".to_string(),
                )
            })?;
            // T-3901 (sprint 39): the literal system+user prompt (+media),
            // recorded once so a later `replay()` doesn't have to re-derive
            // it. Only written for a fresh session — a resumed one's initial
            // prompt already lives in the session it resumed from.
            sink.write_event(Event::SessionPrompt {
                system: system.to_string(),
                user: prompt.to_string(),
                media: args.media.clone(),
            })?;
            (
                vec![
                    Message::system(system),
                    Message::user_with_media(prompt, args.media.clone()),
                ],
                0,
                None,
            )
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
        messages.push(Message::user(extra));
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

    let stop = 'outer: loop {
        if turns >= u32::from(args.policy.max_turns) {
            break StopReason::MaxTurns;
        }
        let turn = turns;
        turns += 1;
        sink.write_event(Event::TurnStart { turn })?;

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
            messages: messages.clone(),
            sampling: args.sampling.clone(),
            tools,
            constraint,
        };
        if let Err(e) = request.validate() {
            sink.write_event(Event::Note {
                text: format!("invalid request constructed by loop: {e}"),
            })?;
            break StopReason::ProviderError;
        }
        sink.write_event(Event::PromptAssembled {
            turn,
            message_count: messages.len() as u32,
            chars: messages
                .iter()
                .map(|m| m.text.as_deref().unwrap_or_default().len() as u64)
                .sum(),
            offered_tools: offered_names.clone(),
        })?;
        // Only ConstrainedJson truly applies a constraint — emit the event
        // honestly (TextXml previously emitted a false ConstraintApplied).
        if args.protocol == ActionProtocol::ConstrainedJson {
            sink.write_event(Event::ConstraintApplied {
                kind: "json_schema".to_string(),
            })?;
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

        sink.write_event(Event::TurnEnd {
            turn,
            text: completion.message.text.clone(),
            tool_call_count: completion.message.tool_calls.len() as u32,
            input_tokens: completion.input_tokens,
            output_tokens: completion.output_tokens,
            // T-3902 (sprint 39): needed to replay the truncation-retry nudge.
            truncated: completion.truncated,
        })?;

        // Constrained truncation (ADR-015): a completion cut off by the token
        // budget cannot be a valid action — do not parse it. Nudge once to
        // re-issue concisely; a second truncation stops the loop. (The
        // partial JSON is NOT added to history; it is recorded in TurnEnd.)
        if args.protocol == ActionProtocol::ConstrainedJson && completion.truncated {
            if truncated_once {
                break StopReason::TruncatedAction;
            }
            truncated_once = true;
            messages.push(truncation_retry_message());
            continue;
        }

        // Best-effort final text is native-mode only: in grammar mode the
        // assistant text IS the action JSON, never a final answer.
        if args.protocol == ActionProtocol::NativeTools
            && let Some(text) = completion.message.text.clone().filter(|t| !t.is_empty())
        {
            last_text = Some(text);
        }
        messages.push(completion.message.clone());

        let (actions, parse_error) = match args.protocol {
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

            let nudge_text = match parse_error {
                Some(e) => format!("XML parse error: {e}. {}", no_action_nudge(args.protocol)),
                None => no_action_nudge(args.protocol).to_string(),
            };
            messages.push(Message::user(nudge_text));
            continue;
        }

        // Repetition guard (hash ALL actions in the turn).
        match repetition.observe(&actions) {
            crate::repetition::Verdict::Proceed => {}
            crate::repetition::Verdict::Warn => {
                sink.write_event(Event::RepetitionGuard {
                    action: "warned".to_string(),
                })?;
                // A direct imperative naming the repeated tool steers small models
                // far better than the old soft/conditional wording — the common
                // small-model failure is repeat-not-terminate (it has the result
                // but doesn't transition to task_complete). See ADR-031.
                let repeated: Vec<&str> = actions.iter().map(|a| a.name.as_str()).collect();
                messages.push(repetition_warn_message(&repeated));
            }
            crate::repetition::Verdict::Stop => {
                sink.write_event(Event::RepetitionGuard {
                    action: "stopped".to_string(),
                })?;
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
                sink.write_event(Event::NoProgressGuard {
                    action: "warned".to_string(),
                })?;
                let repeated: Vec<&str> = actions.iter().map(|a| a.name.as_str()).collect();
                messages.push(no_progress_warn_message(&repeated));
            }
            crate::repetition::Verdict::Stop => {
                sink.write_event(Event::NoProgressGuard {
                    action: "stopped".to_string(),
                })?;
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
                // T-3902 (sprint 39): trace it too (never dispatched/executed)
                // — closes the NativeTools gap where a terminator's summary
                // args were otherwise recorded nowhere in the trace (ConstrainedJson/
                // TextXml already carry them in this turn's raw `TurnEnd.text`).
                // Written INLINE, at this exact loop position, so trace order
                // matches `actions`' original order even when the terminator
                // is mixed among other calls in the same turn.
                sink.write_event(Event::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    args: call.args.clone(),
                })?;
                continue; // other calls in this turn still execute first
            }
            sink.write_event(Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            })?;
            let (result_text, is_error, duration_ms, checks) =
                dispatch(args.registry, args.workspace, &call.name, &call.args);
            dispatched += 1;
            if is_error {
                errored += 1;
            }
            for check in &checks {
                sink.write_event(permission_event(check))?;
            }
            sink.write_event(Event::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                output: result_text.full.clone(),
                is_error,
                duration_ms,
            })?;
            messages.push(result_message(
                args.protocol,
                &call.id,
                &call.name,
                &result_text.for_model,
            ));
        }
        if let Some(summary) = terminate_with {
            last_text = Some(summary);
            break 'outer StopReason::TaskComplete;
        }

        // Repeated-failure guard (keys off RESULTS — runs after dispatch, only
        // on a non-terminating turn). Catches the "different tools, all failing"
        // mode the repetition + no-progress guards reset on (ADR-038).
        if dispatched > 0 {
            match failure.observe_turn(dispatched, errored) {
                crate::repetition::Verdict::Proceed => {}
                crate::repetition::Verdict::Warn => {
                    sink.write_event(Event::FailureGuard {
                        action: "warned".to_string(),
                    })?;
                    messages.push(failure_warn_message());
                }
                crate::repetition::Verdict::Stop => {
                    sink.write_event(Event::FailureGuard {
                        action: "stopped".to_string(),
                    })?;
                    break StopReason::RepeatedFailure;
                }
            }
        }
    };

    sink.write_event(Event::SessionEnd {
        reason: stop.as_str().to_string(),
    })?;

    Ok(LoopOutcome {
        final_text: last_text,
        stop,
        turns,
    })
}

/// `pub(crate)` (T-3903, sprint 39): shared with `replay.rs`, which
/// reconstructs the same no-action nudge for a turn with zero `ToolCall`
/// events — the exact original `TextXml` parse-error text isn't traceable
/// (an accepted, narrow approximation), so replay always uses the bare
/// template, never the `"XML parse error: {e}. "`-prefixed form.
pub(crate) fn no_action_nudge(protocol: ActionProtocol) -> &'static str {
    match protocol {
        ActionProtocol::NativeTools => "Respond with a tool call, or your final answer as text.",
        ActionProtocol::ConstrainedJson => {
            "Respond with a single JSON action: {\"tool\": \"tool_name\", \"args\": { ... }}"
        }
        ActionProtocol::TextXml => {
            "Respond with an XML tool call: <tool_call><name>tool_name</name><args>{\"arg\": \"value\"}</args></tool_call>"
        }
    }
}

/// The truncation-retry nudge (T-3903, sprint 39: extracted so `replay.rs`
/// can reproduce it byte-for-byte for a `TurnEnd.truncated == true` turn).
pub(crate) fn truncation_retry_message() -> Message {
    Message::user("Your last action was cut off by the token limit. Re-issue it more concisely.")
}

/// The repetition guard's "warned" nudge (T-3903, sprint 39: extracted; NOT
/// interchangeable with `no_progress_warn_message` — different wording for a
/// different guard).
pub(crate) fn repetition_warn_message(repeated: &[&str]) -> Message {
    Message::user(format!(
        "You already called {} and have the result — do not call it again. \
         If the task is finished, call task_complete now with a one-sentence summary.",
        repeated.join(", ")
    ))
}

/// The no-progress guard's "warned" nudge (T-3903, sprint 39: extracted).
pub(crate) fn no_progress_warn_message(repeated: &[&str]) -> Message {
    Message::user(format!(
        "You have called {} repeatedly without finishing. If the task is \
         complete, call task_complete now; otherwise use a different tool or \
         arguments that move toward the goal.",
        repeated.join(", ")
    ))
}

/// The repeated-failure guard's "warned" nudge (T-3903, sprint 39: extracted;
/// static text, no arguments — unlike the other two guards' nudges).
pub(crate) fn failure_warn_message() -> Message {
    Message::user(
        "Your last tool call(s) failed. Read the error message and try a \
         different approach, or call task_complete if you cannot proceed.",
    )
}

/// Feed a tool result back to the model. Native mode uses the template's tool
/// role; grammar mode frames it as a user message (the template's tool role
/// may misbehave without `tools` in context — ADR-015). `pub(crate)` (T-3903,
/// sprint 39): shared with `replay.rs`.
pub(crate) fn result_message(
    protocol: ActionProtocol,
    call_id: &str,
    name: &str,
    output: &str,
) -> Message {
    match protocol {
        ActionProtocol::NativeTools => Message::tool_result(call_id, output),
        // No tools are in context for these protocols, so the template's tool
        // role may misbehave — frame results as user messages (ADR-015).
        ActionProtocol::ConstrainedJson | ActionProtocol::TextXml => {
            Message::user(format!("[tool_result for {name}] {output}"))
        }
    }
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
    for_model: String,
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
                for_model: output.for_model,
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
                    for_model: text,
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
                    for_model: text,
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
