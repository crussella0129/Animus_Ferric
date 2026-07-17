use std::path::Path;

use ferric_core::Message;
use ferric_guard::Workspace;
use ferric_loop::{RunArgs, ThreadSleeper};
use ferric_provider::{Completion, MockProvider};
use ferric_trace::{Event, JsonlSink, TraceReader};
use std::process::ExitCode;

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
    let mut system_prompt = String::new();
    let mut initial_user_prompt = String::new();

    let mut workspace_path = String::new();
    let mut policy = ferric_core::RunPolicy {
        tier: ferric_core::Tier::Small,
        protocol: ferric_core::Protocol::ConstrainedJson,
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
            } => {
                protocol = *p;
                policy = ferric_core::RunPolicy {
                    tier: *tier,
                    protocol: ferric_core::Protocol::ConstrainedJson,
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
                let message = Message {
                    role: ferric_core::Role::Assistant,
                    text: text.clone(),
                    tool_calls: current_tool_calls.clone(),
                    tool_call_id: None,
                    media: vec![],
                };
                script.push(Completion {
                    message,
                    input_tokens: *input_tokens,
                    output_tokens: *output_tokens,
                    truncated: *truncated,
                });
                current_tool_calls.clear();
            }
            _ => {}
        }
    }

    let provider = MockProvider::new(script);

    // We cannot construct the original workspace if it doesn't exist, but typically trace verify is run inside the workspace or we create a dummy one.
    // For now, if the original workspace path fails, we fallback to current dir.
    let workspace = Workspace::new(&workspace_path)
        .unwrap_or_else(|_| Workspace::new(std::env::current_dir().unwrap()).unwrap());

    let mut registry = ferric_tools::Registry::new();
    ferric_tools::register_builtin_tools(&mut registry);

    // Temp file for the new trace
    let trace_path = std::env::temp_dir().join("verify.jsonl");
    if trace_path.exists() {
        std::fs::remove_file(&trace_path).unwrap();
    }
    let mut sink = JsonlSink::open(&trace_path, "verify").unwrap();

    let sleeper = ThreadSleeper;

    let args = RunArgs {
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
        taint_set: ferric_guard::TaintSet::new(),
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
