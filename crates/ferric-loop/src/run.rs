use std::time::Duration;

use ferric_core::{FerricError, Message, RunPolicy};
use ferric_guard::Workspace;
use ferric_provider::{CompletionRequest, Provider, SamplingParams, ToolDescriptor};
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

/// The default system prompt. Deliberately tiny — small contexts are the
/// silent killer (s1 research); per-tier prompt assembly via oovra is s2.
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Ferric, a coding agent. \
Use the available tools to act on the workspace. \
When the task is done, call task_complete with a one-sentence summary. \
Never describe a tool call in prose - actually call the tool.";

/// Everything `run` needs. Borrowed so callers own lifecycle and the loop
/// stays executor-agnostic.
pub struct RunArgs<'a> {
    pub provider: &'a dyn Provider,
    pub registry: &'a Registry,
    pub workspace: &'a Workspace,
    pub policy: &'a RunPolicy,
    pub sampling: SamplingParams,
    pub sleeper: &'a dyn Sleeper,
    /// Override the built-in system prompt (None = DEFAULT_SYSTEM_PROMPT).
    pub system_prompt: Option<&'a str>,
}

/// Run the agent loop for one user prompt. Trace I/O errors abort with `Err`;
/// everything else (provider failures included) folds into the outcome.
pub async fn run(
    args: RunArgs<'_>,
    sink: &mut JsonlSink,
    prompt: &str,
) -> Result<LoopOutcome, FerricError> {
    sink.write_event(Event::SessionStart {
        workspace: args.workspace.root().display().to_string(),
    })?;

    let system = args.system_prompt.unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let mut messages = vec![Message::system(system), Message::user(prompt)];

    let tools = offered_tools(args.registry, args.policy);
    let offered_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();

    let mut repetition = crate::repetition::RepetitionGuard::new();
    let mut last_text: Option<String> = None;
    let mut nudged_for_empty = false;
    let mut turns = 0u32;

    let stop = 'outer: loop {
        if turns >= u32::from(args.policy.max_turns) {
            break StopReason::MaxTurns;
        }
        let turn = turns;
        turns += 1;
        sink.write_event(Event::TurnStart { turn })?;

        let request = CompletionRequest {
            messages: messages.clone(),
            sampling: args.sampling.clone(),
            tools: tools.clone(),
            constraint: None,
        };
        // ADR-010 primary enforcement: the loop never sends an invalid shape.
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

        let completion =
            match crate::backoff::complete_with_backoff(args.provider, request, args.sleeper).await
            {
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
        })?;

        if let Some(text) = completion.message.text.clone().filter(|t| !t.is_empty()) {
            last_text = Some(text);
        }
        messages.push(completion.message.clone());

        if completion.message.tool_calls.is_empty() {
            match &completion.message.text {
                Some(text) if !text.trim().is_empty() => break StopReason::FinalText,
                _ => {
                    // Empty completion: nudge once, then stop.
                    if nudged_for_empty {
                        break StopReason::EmptyCompletion;
                    }
                    nudged_for_empty = true;
                    messages.push(Message::user(
                        "Respond with a tool call, or your final answer as text.",
                    ));
                    continue;
                }
            }
        }

        // Repetition guard (hash ALL calls in the turn).
        match repetition.observe(&completion.message.tool_calls) {
            crate::repetition::Verdict::Proceed => {}
            crate::repetition::Verdict::Warn => {
                sink.write_event(Event::RepetitionGuard {
                    action: "warned".to_string(),
                })?;
                messages.push(Message::user(
                    "You are repeating the same tool calls. Take a different action, \
                     or call task_complete if the task is done.",
                ));
            }
            crate::repetition::Verdict::Stop => {
                sink.write_event(Event::RepetitionGuard {
                    action: "stopped".to_string(),
                })?;
                break StopReason::RepetitionGuard;
            }
        }

        // Dispatch tool calls in order; intercept the terminator.
        let mut terminate_with: Option<String> = None;
        for call in &completion.message.tool_calls {
            if crate::terminator::is_task_complete(&call.name) {
                terminate_with = Some(crate::terminator::summary_of(&call.args));
                continue; // other calls in this turn still execute first
            }
            sink.write_event(Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            })?;
            let (result_text, is_error, duration_ms, checks) =
                dispatch(args.registry, args.workspace, &call.name, &call.args);
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
            messages.push(Message::tool_result(&call.id, &result_text.for_model));
        }
        if let Some(summary) = terminate_with {
            last_text = Some(summary);
            break 'outer StopReason::TaskComplete;
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

/// The tools offered to the model: the policy-filtered registry set plus the
/// always-offered `task_complete` terminator (exempt from `max_tools`).
fn offered_tools(registry: &Registry, policy: &RunPolicy) -> Vec<ToolDescriptor> {
    let mut tools: Vec<ToolDescriptor> = registry
        .tools_for_policy(policy)
        .into_iter()
        .map(|spec| ToolDescriptor {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        })
        .collect();
    tools.push(crate::terminator::descriptor());
    tools
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
