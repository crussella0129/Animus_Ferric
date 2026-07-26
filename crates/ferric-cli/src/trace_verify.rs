use std::path::Path;

use ferric_core::Message;
use ferric_guard::Workspace;
use ferric_loop::{RunArgs, ThreadSleeper};
use ferric_provider::{Completion, MockProvider};
use ferric_trace::{Event, JsonlSink, TraceReader};
use std::process::ExitCode;

/// A turn whose `TurnEnd` has been seen but whose dispatched tool calls have
/// not all arrived yet. See the `TurnEnd` arm below for why that gap exists.
struct PendingCompletion {
    text: Option<String>,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    truncated: bool,
}

/// Close an open turn into a scripted `Completion`, pairing it with the tool
/// calls traced since its `TurnEnd`.
fn close_turn(
    pending: &mut Option<PendingCompletion>,
    tool_calls: &mut Vec<ferric_core::ToolCall>,
    script: &mut Vec<Completion>,
) {
    if let Some(p) = pending.take() {
        script.push(Completion {
            message: Message {
                role: ferric_core::Role::Assistant,
                text: p.text,
                tool_calls: std::mem::take(tool_calls),
                tool_call_id: None,
                media: vec![],
            },
            input_tokens: p.input_tokens,
            output_tokens: p.output_tokens,
            truncated: p.truncated,
        });
    }
    tool_calls.clear();
}

pub fn trace_verify(golden: &Path) -> ExitCode {
    let events = match TraceReader::open(golden) {
        Ok(reader) => {
            let mut evs = Vec::new();
            for item in reader {
                match item {
                    Ok(pe) => {
                        if let ferric_trace::ParsedEvent::Known(ev) = pe.event {
                            evs.push(ev);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading trace: {e}");
                        std::process::exit(1);
                    }
                }
            }
            evs
        }
        Err(e) => {
            eprintln!("Failed to open trace: {e}");
            std::process::exit(1);
        }
    };

    let mut script = Vec::new();
    let mut current_tool_calls = Vec::new();
    let mut pending_turn: Option<PendingCompletion> = None;
    let mut system_prompt = String::new();
    let mut initial_user_prompt = String::new();

    let mut workspace_path = String::new();
    let mut policy = ferric_core::RunPolicy {
        tier: ferric_core::Tier::Small,
        uses_planner: false,
        max_plan_steps: 0,
        max_turns_per_step: 0,
        allows_subagents: false,
        max_turns: 10,
        max_tools: 10,
        prompt_budget_tokens: 8192,
        max_output_tokens: 4096,
        max_ring: None,
        compact_trigger_fraction: 0.85,
        compact_keep_last_turns: 2,
    };
    let mut protocol = ferric_core::ActionProtocol::NativeTools;
    // Restored from the trace alongside the rest of the policy. Re-running a
    // trace under a *different* cap than it was recorded with would rebuild a
    // different context window and report a mismatch that belongs to the
    // verifier, not the run (ADR-093).
    let mut truncation_limit = ferric_core::DEFAULT_TRUNCATION_LIMIT;

    for event in &events {
        match event {
            Event::SessionStart { workspace, .. } => {
                workspace_path = workspace.clone();
            }
            Event::PolicySelected {
                tier,
                protocol: p,
                max_turns,
                max_tools,
                prompt_budget_tokens,
                max_output_tokens,
                truncation_limit: cap,
            } => {
                protocol = *p;
                truncation_limit = *cap;
                policy = ferric_core::RunPolicy {
                    tier: *tier,
                    uses_planner: false,
                    max_plan_steps: 0,
                    max_turns_per_step: 0,
                    allows_subagents: false,
                    max_turns: *max_turns as u8,
                    max_tools: *max_tools as u8,
                    prompt_budget_tokens: *prompt_budget_tokens,
                    max_output_tokens: *max_output_tokens,
                    max_ring: None,
                    compact_trigger_fraction: 0.85,
                    compact_keep_last_turns: 2,
                };
            }
            Event::SessionPrompt {
                system,
                user,
                media: _,
            } => {
                system_prompt = system.clone();
                initial_user_prompt = user.clone();
            }
            Event::ToolCall { id, name, args } => {
                current_tool_calls.push(ferric_core::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    args: args.clone(),
                });
            }
            Event::TurnEnd {
                text,
                input_tokens,
                output_tokens,
                truncated,
                ..
            } => {
                // Hold the turn open rather than closing it here. `run()`
                // writes `TurnEnd` BEFORE dispatching, so a turn's own
                // `ToolCall` events land *after* it (confirmed in a real
                // trace: turn_end then tool_call). Building the completion at
                // `TurnEnd` therefore gave each turn the PREVIOUS turn's
                // calls, and dropped the final turn's entirely — which is how
                // the terminator went missing from every replayed script.
                // `replay()` gets this right by committing a turn only once a
                // later event proves dispatch finished; this now uses the same
                // rule. ADR-093.
                pending_turn = Some(PendingCompletion {
                    text: text.clone(),
                    input_tokens: *input_tokens,
                    output_tokens: *output_tokens,
                    truncated: *truncated,
                });
            }
            Event::TurnStart { .. } | Event::SessionEnd { .. } => {
                close_turn(&mut pending_turn, &mut current_tool_calls, &mut script);
            }
            _ => {}
        }
    }
    // A trace cut short by a kill has no closing event; its last turn is still
    // a completion the model produced.
    close_turn(&mut pending_turn, &mut current_tool_calls, &mut script);

    let provider = MockProvider::new(script);

    // We cannot construct the original workspace if it doesn't exist, but typically trace verify is run inside the workspace or we create a dummy one.
    // For now, if the original workspace path fails, we fallback to current dir.
    let workspace = Workspace::new(&workspace_path)
        .unwrap_or_else(|_| Workspace::new(std::env::current_dir().unwrap()).unwrap());

    let mut registry = ferric_tools::Registry::with_truncation_limit(truncation_limit);
    ferric_tools::register_builtin_tools(&mut registry);

    // Temp file for the new trace
    let trace_path = std::env::temp_dir().join("verify.jsonl");
    if trace_path.exists() {
        std::fs::remove_file(&trace_path).unwrap();
    }
    let mut sink = JsonlSink::open(&trace_path, "verify").unwrap();

    let sleeper = ThreadSleeper;

    let args = RunArgs {
        edit_approver: None,
        cancel_flag: None,
        provider: &provider,
        registry: &registry,
        workspace: &workspace,
        policy: &policy,
        protocol,
        sampling: ferric_provider::SamplingParams::default(),
        sleeper: &sleeper,
        system_prompt: Some(&system_prompt),
        prompt_lineage: None,
        media: vec![],
        stream_sink: None,
        resume: None,
        provenance: ferric_guard::Provenance::Clean,
        sink_policy: ferric_guard::SinkPolicy::deny(),
        hooks: None,
    };

    let outcome = futures_executor::block_on(ferric_loop::run(
        args,
        &mut sink,
        Some(&initial_user_prompt),
    ));

    match outcome {
        Ok(_) => {
            // Compare traces
            let new_reader = TraceReader::open(&trace_path).unwrap();
            let new_events: Vec<_> = new_reader
                .into_iter()
                .filter_map(|e| e.ok())
                .map(|pe| pe.event)
                .filter_map(|e| {
                    if let ferric_trace::ParsedEvent::Known(ev) = e {
                        Some(ev)
                    } else {
                        None
                    }
                })
                .collect();

            if new_events.len() != events.len() {
                eprintln!(
                    "Mismatch in number of events: {} vs golden {}",
                    new_events.len(),
                    events.len()
                );
                std::process::exit(1);
            }
            for (i, (new_event, golden_event)) in new_events.iter().zip(events.iter()).enumerate() {
                if std::mem::discriminant(new_event) != std::mem::discriminant(golden_event) {
                    eprintln!(
                        "Event mismatch at index {}: {:?} vs {:?}",
                        i, new_event, golden_event
                    );
                    std::process::exit(1);
                }
            }
            println!("Trace verification successful.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Verify failed: {}", e);
            ExitCode::FAILURE
        }
    }
}
