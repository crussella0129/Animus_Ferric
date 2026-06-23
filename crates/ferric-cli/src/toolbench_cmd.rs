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

/// The classified outcome of one toolbench iteration. This is what turns the
/// bench from a pass/fail counter into a diagnostic: it says *why* a model
/// missed, so a user can judge whether a smaller model is still good enough.
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Called the target tool with every schema-required arg present.
    Success,
    /// Called a real, but different, tool.
    WrongTool(String),
    /// Called the target tool but a schema-required arg is missing.
    MalformedArgs,
    /// Produced no parseable action (native: no tool_calls; text: empty).
    NoAction,
    /// Produced non-empty action-shaped text that did not parse.
    ParseError,
}

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
impl Outcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Success)
    }
}

/// Classify one completion against the `target` tool (and its `schema`) using
/// the SAME parser the agent loop uses for `protocol`. Distinguishes "nothing"
/// (NoAction) from "action-shaped but unparseable" (ParseError), "called a
/// different tool" (WrongTool), and "right tool, missing a required arg"
/// (MalformedArgs). The arg check is a lightweight required-keys check against
/// `schema.required`, not full JSON-Schema validation.
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
pub fn classify(
    protocol: ActionProtocol,
    completion: &Completion,
    target: &str,
    schema: &serde_json::Value,
) -> Outcome {
    let text = completion.message.text.as_deref().unwrap_or_default();
    let parsed: Result<Option<ToolCall>, ()> = match protocol {
        ActionProtocol::NativeTools => Ok(completion.message.tool_calls.first().cloned()),
        ActionProtocol::ConstrainedJson => {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                ferric_loop::parse_json_action(0, text)
                    .map(Some)
                    .map_err(|_| ())
            }
        }
        ActionProtocol::TextXml => {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                ferric_loop::parse_action(0, text).map(Some).map_err(|_| ())
            }
        }
    };
    let call = match parsed {
        Ok(Some(call)) => call,
        Ok(None) => return Outcome::NoAction,
        Err(()) => return Outcome::ParseError,
    };
    if call.name != target {
        return Outcome::WrongTool(call.name);
    }
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for key in required.iter().filter_map(|k| k.as_str()) {
            if call.args.get(key).is_none() {
                return Outcome::MalformedArgs;
            }
        }
    }
    Outcome::Success
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

                let outcome = classify(protocol, &completion, &tool.name, &tool.input_schema);
                let pass = outcome.is_success();
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

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }

    fn native_completion(name: &str, args: serde_json::Value) -> Completion {
        Completion {
            message: Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "t".to_string(),
                    name: name.to_string(),
                    args,
                }],
                tool_call_id: None,
            },
            input_tokens: None,
            output_tokens: None,
            truncated: false,
        }
    }

    #[test]
    fn outcome_is_success() {
        assert!(Outcome::Success.is_success());
        assert!(!Outcome::NoAction.is_success());
        assert!(!Outcome::WrongTool("x".to_string()).is_success());
    }

    #[test]
    fn classify_success_native() {
        let c = native_completion("read_file", json!({"path": "x"}));
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::Success
        );
    }

    #[test]
    fn classify_success_constrained() {
        let c = text_completion(r#"{"tool":"read_file","args":{"path":"x"}}"#);
        assert_eq!(
            classify(ActionProtocol::ConstrainedJson, &c, "read_file", &schema()),
            Outcome::Success
        );
    }

    #[test]
    fn classify_wrong_tool() {
        let c = native_completion("write_file", json!({"path": "x"}));
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::WrongTool("write_file".to_string())
        );
    }

    #[test]
    fn classify_malformed_args() {
        // Right tool, but the required "path" arg is missing.
        let c = native_completion("read_file", json!({}));
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::MalformedArgs
        );
    }

    #[test]
    fn classify_no_action() {
        // Native: tool_calls empty (the model chatted instead of calling).
        let c = text_completion("I cannot help with that.");
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::NoAction
        );
    }

    #[test]
    fn classify_parse_error() {
        // ConstrainedJson: non-empty text that is not a valid action object.
        let bad_json = text_completion(r#"{"tool": "read_file", "args": {oops"#);
        assert_eq!(
            classify(
                ActionProtocol::ConstrainedJson,
                &bad_json,
                "read_file",
                &schema()
            ),
            Outcome::ParseError
        );
        // TextXml: prose with no <tool_call> is a parse failure too.
        let prose = text_completion("just some prose");
        assert_eq!(
            classify(ActionProtocol::TextXml, &prose, "read_file", &schema()),
            Outcome::ParseError
        );
    }
}
