//! `ferric toolbench` — per-tool fire-rate benchmark.
//!
//! Measures, for each registered tool, how often the model emits a call to
//! exactly that tool. Critically (ADR-022) it parses the completion with the
//! SAME parser the agent loop uses for the active protocol — native
//! `tool_calls`, constrained `{tool,args}` JSON, or scraped `<tool_call>` XML —
//! so the fire rate reflects the real path, not a native-only check that always
//! read empty (the s6 0.0% bug).

use clap::Args;
use std::process::ExitCode;

use crate::backend::BackendOpts;
use crate::query::ProtocolArg;

// Used by `extract_action` (and its tests); excluded from the backend-free,
// non-test bin build so it stays unused-import-clean under `-D warnings`.
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
use ferric_core::{ActionProtocol, ToolCall};
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
use ferric_provider::Completion;

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
use {
    crate::backend::create_provider,
    ferric_core::{Message, ModelProfile, policy_for},
    ferric_loop::{action_schema, select_protocol},
    ferric_provider::{CompletionRequest, Constraint, SamplingParams, ToolDescriptor},
    ferric_tools::Registry,
};

#[derive(Args)]
pub struct ToolbenchArgs {
    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Action protocol to test (default: chosen from the backend's capabilities)
    #[arg(long, value_enum)]
    pub protocol: Option<ProtocolArg>,

    /// Number of iterations per tool to test fire rate
    #[arg(long, default_value_t = 10)]
    pub iterations: u32,
}

/// Extract the fired tool call from a completion using the SAME parser the
/// agent loop uses for `protocol`: native `tool_calls`, constrained
/// `{tool,args}` JSON, or scraped `<tool_call>` XML. This is what makes the
/// bench measure the path the agent actually runs. Gated to feature/test builds
/// so the backend-free `cargo build` stays dead-code-clean.
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
pub fn extract_action(protocol: ActionProtocol, completion: &Completion) -> Option<ToolCall> {
    let text = completion.message.text.as_deref().unwrap_or_default();
    match protocol {
        ActionProtocol::NativeTools => completion.message.tool_calls.first().cloned(),
        ActionProtocol::ConstrainedJson => ferric_loop::parse_json_action(0, text).ok(),
        ActionProtocol::TextXml => ferric_loop::parse_action(0, text).ok(),
    }
}

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
fn build_request(
    protocol: ActionProtocol,
    all_tools: &[ToolDescriptor],
    schema: &serde_json::Value,
    tool_name: &str,
) -> CompletionRequest {
    let system_prompt = match protocol {
        ActionProtocol::NativeTools => {
            "You are a tool-calling assistant. Call exactly the tool the user names, with \
             valid arguments. Output only the tool call."
        }
        ActionProtocol::ConstrainedJson => {
            "Respond with exactly one JSON object and nothing else: \
             {\"tool\": \"<name>\", \"args\": { ... }}."
        }
        ActionProtocol::TextXml => {
            "Respond with exactly one XML tool call: \
             <tool_call><name>TOOL</name><args>{\"arg\": \"value\"}</args></tool_call>."
        }
    };
    let user_prompt =
        format!("Invoke the `{tool_name}` tool with dummy data that matches its schema.");
    // ADR-010: constraint XOR tools, by construction.
    let (tools, constraint) = match protocol {
        ActionProtocol::NativeTools => (all_tools.to_vec(), None),
        ActionProtocol::ConstrainedJson => {
            (Vec::new(), Some(Constraint::JsonSchema(schema.clone())))
        }
        ActionProtocol::TextXml => (Vec::new(), None),
    };
    CompletionRequest {
        messages: vec![Message::system(system_prompt), Message::user(user_prompt)],
        tools,
        sampling: SamplingParams {
            temperature: 0.0,
            max_tokens: 256,
            ..SamplingParams::default()
        },
        constraint,
    }
}

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
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
                input_schema: spec.input_schema,
            })
            .collect();

        // Protocol from the backend's real capabilities (an explicit
        // `--protocol` overrides), so the bench measures what `ferric query`
        // would actually run against this backend.
        let protocol = select_protocol(
            &policy,
            &provider.capabilities(),
            args.protocol.map(ActionProtocol::from),
        );
        let schema = action_schema(&all_tools);

        println!(
            "Toolbench: {} tools x {} iterations | backend={} protocol={:?}",
            all_tools.len(),
            args.iterations,
            provider.id(),
            protocol,
        );

        let mut overall_successes = 0u32;
        let mut overall_total = 0u32;

        for tool in &all_tools {
            let mut successes = 0u32;
            print!("Testing tool '{:<15}': ", tool.name);

            for _ in 0..args.iterations {
                let request = build_request(protocol, &all_tools, &schema, &tool.name);
                let completion = provider
                    .complete(request)
                    .await
                    .map_err(|e| format!("provider error: {e}"))?;

                let pass =
                    extract_action(protocol, &completion).is_some_and(|tc| tc.name == tool.name);
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
            println!(" [{successes} / {}] ({fire_rate:.1}%)", args.iterations);
        }

        println!("\n=== Final Fire Rate Report (protocol={protocol:?}) ===");
        let overall_rate = (overall_successes as f64 / overall_total as f64) * 100.0;
        println!("Overall accuracy: {overall_rate:.1}% ({overall_successes} / {overall_total})");

        Ok::<(), String>(())
    });

    if let Err(e) = result {
        eprintln!("toolbench error: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(not(any(feature = "backend-mistralrs", feature = "backend-openai")))]
pub fn run_toolbench(_args: ToolbenchArgs) -> ExitCode {
    eprintln!(
        "this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai`"
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::{Message, Role};
    use serde_json::json;

    fn text_completion(text: &str) -> Completion {
        Completion {
            message: Message::assistant(text),
            input_tokens: None,
            output_tokens: None,
            truncated: false,
        }
    }

    fn native_completion(name: &str) -> Completion {
        Completion {
            message: Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "t".to_string(),
                    name: name.to_string(),
                    args: json!({}),
                }],
                tool_call_id: None,
            },
            input_tokens: None,
            output_tokens: None,
            truncated: false,
        }
    }

    #[test]
    fn native_path_reads_tool_calls() {
        let c = native_completion("read_file");
        assert_eq!(
            extract_action(ActionProtocol::NativeTools, &c)
                .unwrap()
                .name,
            "read_file"
        );
    }

    #[test]
    fn constrained_path_parses_json() {
        let c = text_completion(r#"{"tool":"read_file","args":{"path":"x"}}"#);
        assert_eq!(
            extract_action(ActionProtocol::ConstrainedJson, &c)
                .unwrap()
                .name,
            "read_file"
        );
    }

    #[test]
    fn textxml_path_scrapes_xml() {
        let c = text_completion(
            "<tool_call><name>read_file</name><args>{\"path\":\"x\"}</args></tool_call>",
        );
        assert_eq!(
            extract_action(ActionProtocol::TextXml, &c).unwrap().name,
            "read_file"
        );
    }

    #[test]
    fn no_action_is_a_miss() {
        let c = text_completion("I cannot help with that.");
        assert!(extract_action(ActionProtocol::ConstrainedJson, &c).is_none());
        assert!(extract_action(ActionProtocol::TextXml, &c).is_none());
        assert!(extract_action(ActionProtocol::NativeTools, &c).is_none());
    }
}
