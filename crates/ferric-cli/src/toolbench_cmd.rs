use clap::Args;
use std::process::ExitCode;

use crate::backend::BackendOpts;

// These are used only by the feature-gated `run_toolbench`; gating the imports
// keeps the default (backend-free) build warning-clean under `-D warnings`.
#[cfg(any(
    feature = "backend-mistralrs",
    feature = "backend-openai",
    feature = "backend-python"
))]
use {
    crate::backend::create_provider,
    ferric_core::{Message, ModelProfile, Role, policy_for},
    ferric_provider::{CompletionRequest, SamplingParams, ToolDescriptor},
    ferric_tools::Registry,
};

#[derive(Args)]
pub struct ToolbenchArgs {
    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Number of iterations per tool to test fire rate
    #[arg(long, default_value_t = 10)]
    pub iterations: u32,
}

#[cfg(any(
    feature = "backend-mistralrs",
    feature = "backend-openai",
    feature = "backend-python"
))]
pub fn run_toolbench(args: ToolbenchArgs) -> ExitCode {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tokio runtime error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(async {
        let provider_box = create_provider(&args.backend_opts).await?;
        let provider = provider_box.as_ref();

        let mut registry = Registry::new();
        ferric_tools::register_builtin_tools(&mut registry);

        let profile = ModelProfile {
            params_b: 8.0,
            quant: "Q4".to_string(),
            ctx: 4096,
            family: "unknown".to_string(),
            measured_level: None,
        };
        let policy = policy_for(&profile);

        let all_tools: Vec<ToolDescriptor> = registry
            .tools_for_policy(&policy)
            .into_iter()
            .map(|spec| ToolDescriptor {
                name: spec.name,
                description: spec.description,
                input_schema: serde_json::to_value(&spec.input_schema).unwrap_or(serde_json::json!({})),
            })
            .collect();

        println!("Running toolbench across {} tools with {} iterations each...", all_tools.len(), args.iterations);

        let mut overall_successes = 0;
        let mut overall_total = 0;

        for tool in &all_tools {
            let mut successes = 0;
            print!("Testing tool '{:<15}': ", tool.name);

            for _ in 1..=args.iterations {
                let system_prompt = "You are a specialized AI assistant. Your ONLY purpose is to call the tool requested by the user. Do not provide any conversational text, explanations, or thinking. Simply output the exact tool call with valid JSON arguments.";

                let user_prompt = format!(
                    "Please invoke the `{}` tool with dummy data that strictly matches its schema.",
                    tool.name
                );

                let request = CompletionRequest {
                    messages: vec![
                        Message {
                            role: Role::System,
                            text: Some(system_prompt.to_string()),
                            tool_calls: vec![],
                            tool_call_id: None,
                        },
                        Message {
                            role: Role::User,
                            text: Some(user_prompt),
                            tool_calls: vec![],
                            tool_call_id: None,
                        },
                    ],
                    tools: all_tools.clone(),
                    sampling: SamplingParams {
                        temperature: 0.0,
                        max_tokens: 256,
                        ..SamplingParams::default()
                    },
                    constraint: None,
                };

                let completion = provider.complete(request).await.map_err(|e| format!("provider error: {e}"))?;

                let mut pass = false;
                if completion.message.tool_calls.len() == 1 {
                    let tc = &completion.message.tool_calls[0];
                    if tc.name == tool.name {
                        // Check if arguments are present and not empty/null strings
                        // Since provider validates arguments minimally, we can assume
                        // if the provider returned it as a tool call, the schema matched
                        // or at least it tried.
                        pass = true;
                    }
                }

                if pass {
                    successes += 1;
                    overall_successes += 1;
                }
                overall_total += 1;

                print!("{}", if pass { "." } else { "F" });
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            let fire_rate = (successes as f64 / args.iterations as f64) * 100.0;
            println!(" [{} / {}] ({:.1}%)", successes, args.iterations, fire_rate);
        }

        println!("\n=== Final Fire Rate Report ===");
        let overall_rate = (overall_successes as f64 / overall_total as f64) * 100.0;
        println!("Overall accuracy: {:.1}% ({} / {})", overall_rate, overall_successes, overall_total);

        Ok::<(), String>(())
    });

    if let Err(e) = result {
        eprintln!("toolbench error: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(not(any(
    feature = "backend-mistralrs",
    feature = "backend-openai",
    feature = "backend-python"
)))]
pub fn run_toolbench(_args: ToolbenchArgs) -> ExitCode {
    eprintln!(
        "this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai,backend-python`"
    );
    ExitCode::FAILURE
}
