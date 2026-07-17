#![allow(unused_imports)]
use clap::Args;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::backend::BackendOpts;
use ferric_trace::TraceReader;

#[cfg(feature = "backend-openai")]
use crate::backend::create_provider;
#[cfg(feature = "backend-openai")]
use ferric_provider::CompletionRequest;

#[derive(Args, Clone)]
pub struct DreamArgs {
    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Number of recent traces to parse
    #[arg(long, default_value_t = 5)]
    pub recent_traces: usize,

    /// Path to the output memory file
    #[arg(long, default_value = ".ferric/MEMORY.md")]
    pub memory_file: PathBuf,
}

pub fn run_dream(args: DreamArgs) -> ExitCode {
    #[cfg(feature = "backend-openai")]
    {
        futures_executor::block_on(async {
            match dream_inner(args).await {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("dream failed: {}", e);
                    ExitCode::FAILURE
                }
            }
        })
    }
    #[cfg(not(feature = "backend-openai"))]
    {
        let _ = args;
        eprintln!("Error: 'ferric dream' requires the 'backend-openai' feature to be enabled.");
        ExitCode::FAILURE
    }
}

#[cfg(feature = "backend-openai")]
async fn dream_inner(args: DreamArgs) -> Result<(), String> {
    // 1. Setup provider
    let provider = create_provider(&args.backend_opts).await?;

    // 2. Find traces
    let traces_dir = PathBuf::from(".ferric/traces");
    if !traces_dir.exists() {
        return Err("No .ferric/traces directory found.".to_string());
    }

    let mut entries = fs::read_dir(traces_dir)
        .map_err(|e| format!("failed to read traces dir: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        .collect::<Vec<_>>();

    // Sort by modified time (descending)
    entries.sort_by(|a, b| {
        let meta_a = a
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let meta_b = b
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        meta_b.cmp(&meta_a)
    });

    let to_process = entries
        .into_iter()
        .take(args.recent_traces)
        .collect::<Vec<_>>();
    if to_process.is_empty() {
        return Err("No traces found to dream about.".to_string());
    }

    // 3. Extract contents
    let mut combined_trace_text = String::new();
    for entry in to_process.iter().rev() {
        let path = entry.path();
        combined_trace_text.push_str(&format!("--- TRACE FILE: {} ---\n", path.display()));

        let reader = TraceReader::open(&path).map_err(|e| format!("cannot open trace: {e}"))?;
        for record_res in reader {
            let record = record_res.map_err(|e| format!("bad trace line: {e}"))?;
            match record.event {
                ferric_trace::ParsedEvent::Known(ferric_trace::Event::SessionPrompt {
                    user,
                    ..
                }) => {
                    combined_trace_text.push_str(&format!("User Request: {}\n", user));
                }
                ferric_trace::ParsedEvent::Known(ferric_trace::Event::TurnEnd {
                    text,
                    tool_call_count,
                    ..
                }) => {
                    if let Some(t) = text {
                        combined_trace_text.push_str(&format!("Agent Thought: {}\n", t));
                    }
                    if tool_call_count > 0 {
                        combined_trace_text
                            .push_str(&format!("Agent called {} tools.\n", tool_call_count));
                    }
                }
                ferric_trace::ParsedEvent::Known(ferric_trace::Event::SessionEnd { reason }) => {
                    combined_trace_text.push_str(&format!("Session Ended: {:?}\n", reason));
                }
                _ => {}
            }
        }
        combined_trace_text.push('\n');
    }

    // 4. Load existing memory if any
    let existing_memory = if args.memory_file.exists() {
        fs::read_to_string(&args.memory_file).unwrap_or_default()
    } else {
        String::new()
    };

    // 5. Construct prompt
    let system_prompt = "You are Ferric's Dream Worker. Your task is to analyze raw agent traces from recent sessions and extract high-value insights, architectural patterns, and user preferences into a persistent knowledge base. Consolidate these new insights with any existing memory. Format as Markdown. Be concise but comprehensive.";

    let user_prompt = format!(
        "EXISTING MEMORY:\n{}\n\nRECENT TRACES:\n{}\n\nSynthesize the above into an updated, single, comprehensive MEMORY.md document. DO NOT include conversational filler, just the raw markdown document.",
        if existing_memory.is_empty() {
            "None"
        } else {
            &existing_memory
        },
        combined_trace_text
    );

    // 6. Call LLM
    println!("Dreaming about {} traces...", to_process.len());

    let messages = vec![
        ferric_core::Message {
            role: ferric_core::Role::System,
            text: Some(system_prompt.to_string()),
            media: vec![],
            tool_call_id: None,
            tool_calls: vec![],
        },
        ferric_core::Message {
            role: ferric_core::Role::User,
            text: Some(user_prompt),
            media: vec![],
            tool_call_id: None,
            tool_calls: vec![],
        },
    ];

    let request = CompletionRequest {
        messages,
        tools: vec![],
        constraint: None,
        sampling: ferric_provider::SamplingParams::default(),
    };

    let completion = provider
        .complete(request, None)
        .await
        .map_err(|e| format!("provider error: {:?}", e))?;
    let output = completion.message.text.unwrap_or_default();

    // 7. Write out
    if let Some(parent) = args.memory_file.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&args.memory_file, output).map_err(|e| e.to_string())?;

    println!(
        "Successfully dreamed! Memory saved to {}",
        args.memory_file.display()
    );
    Ok(())
}
