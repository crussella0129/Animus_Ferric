//! In-process mistral.rs backend (feature `backend-mistralrs`).
//!
//! Per `sprints/s1/sprint-research/mistralrs-integration-spec.md`, verified
//! against the published 0.8.1 source. The pure-Rust flagship path (ADR-001):
//! local GGUF only, zero network (local paths short-circuit the HF API;
//! `TokenSource::None` as belt-and-braces — callers should also set
//! `HF_HUB_OFFLINE=1` before any threads spawn).
//!
//! API drift vs the spec (which read master/0.8.3): 0.8.1's `Function` has no
//! `strict` field, so strict tool-calling is not requested here; argument
//! validation falls to the loop's dispatch layer until the dep is bumped.
//!
//! Real-GGUF validation policy (ADR-009): any change to this module requires
//! a traced real-model run (the L0 smoke) before merge.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;

use ferric_core::{Message, Role, ToolCall};

use crate::traits::Provider;
use crate::types::{
    Capabilities, Completion, CompletionRequest, Constraint, ProviderError, SamplingParams,
    ToolDescriptor,
};

pub struct MistralRsConfig {
    /// Directory containing the GGUF file.
    pub model_dir: PathBuf,
    /// GGUF file name inside `model_dir`.
    pub model_file: String,
    /// Literal Jinja template or path to a template file, for GGUFs without
    /// an embedded `tokenizer.chat_template`.
    pub chat_template: Option<String>,
    /// Scheduler sequence slots; small for a single-user harness (default 2).
    pub max_num_seqs: usize,
    /// Force CPU even in a CUDA-featured build (default true: CPU-first, ADR-004).
    pub force_cpu: bool,
}

impl MistralRsConfig {
    pub fn new(model_dir: impl Into<PathBuf>, model_file: impl Into<String>) -> Self {
        Self {
            model_dir: model_dir.into(),
            model_file: model_file.into(),
            chat_template: None,
            max_num_seqs: 2,
            force_cpu: true,
        }
    }
}

/// The in-process mistral.rs provider. Load once (heavy: spawns the engine on
/// its own OS thread with its own runtime), then share.
pub struct MistralRsProvider {
    model: mistralrs::Model,
}

impl MistralRsProvider {
    pub async fn load(config: MistralRsConfig) -> Result<Self, ProviderError> {
        let mut builder = mistralrs::GgufModelBuilder::new(
            config.model_dir.display().to_string(),
            vec![config.model_file.clone()],
        )
        .with_token_source(mistralrs::TokenSource::None)
        .with_max_num_seqs(config.max_num_seqs);
        if config.force_cpu {
            builder = builder.with_force_cpu();
        }
        if let Some(template) = &config.chat_template {
            builder = builder.with_chat_template(template);
        }
        let model = builder
            .build()
            .await
            .map_err(|e| ProviderError::Backend(format!("model load: {e:#}")))?;
        Ok(Self { model })
    }
}

#[async_trait]
impl Provider for MistralRsProvider {
    fn id(&self) -> &str {
        "mistralrs"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_constraint: true,
            supports_native_tool_calls: true,
            exposes_logits: false,
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ProviderError> {
        // ADR-010 defense in depth: reject before contacting the engine.
        request.validate()?;

        let mut builder = map_messages(&request.messages);
        builder = apply_sampling(builder, &request.sampling);
        if !request.tools.is_empty() {
            builder = builder
                .set_tools(map_tools(&request.tools))
                .set_tool_choice(mistralrs::ToolChoice::Auto);
        } else if let Some(constraint) = &request.constraint {
            builder = builder.set_constraint(map_constraint(constraint));
        }

        let response = self
            .model
            .send_chat_request(builder)
            .await
            .map_err(|e| classify_anyhow(&format!("{e:#}")))?;

        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::Backend("response carried no choices".to_string()))?;

        Ok(Completion {
            message: Message {
                role: Role::Assistant,
                text: choice.message.content,
                tool_calls: choice
                    .message
                    .tool_calls
                    .unwrap_or_default()
                    .iter()
                    .map(|tc| ToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        args: parse_args(&tc.function.arguments),
                    })
                    .collect(),
                tool_call_id: None,
            },
            input_tokens: Some(response.usage.prompt_tokens as u32),
            output_tokens: Some(response.usage.completion_tokens as u32),
        })
    }
}

// ---- mapping layer: free functions so they unit-test without a model ----

pub(crate) fn map_constraint(constraint: &Constraint) -> mistralrs::Constraint {
    match constraint {
        Constraint::JsonSchema(schema) => mistralrs::Constraint::JsonSchema(schema.clone()),
        Constraint::Regex(regex) => mistralrs::Constraint::Regex(regex.clone()),
        Constraint::Lark(grammar) => mistralrs::Constraint::Lark(grammar.clone()),
    }
}

pub(crate) fn map_tools(tools: &[ToolDescriptor]) -> Vec<mistralrs::Tool> {
    tools
        .iter()
        .map(|tool| mistralrs::Tool {
            tp: mistralrs::ToolType::Function,
            function: mistralrs::Function {
                name: tool.name.clone(),
                description: Some(tool.description.clone()),
                parameters: schema_to_parameters(&tool.input_schema),
            },
        })
        .collect()
}

/// mistral.rs wants `Option<HashMap<String, Value>>`; our schemas are JSON
/// objects, so this is a faithful reshape (non-object schemas → None).
fn schema_to_parameters(schema: &serde_json::Value) -> Option<HashMap<String, serde_json::Value>> {
    schema
        .as_object()
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

pub(crate) fn map_messages(messages: &[Message]) -> mistralrs::RequestBuilder {
    let mut builder = mistralrs::RequestBuilder::new();
    for message in messages {
        match message.role {
            Role::System => {
                builder = builder.add_message(
                    mistralrs::TextMessageRole::System,
                    message.text.as_deref().unwrap_or_default(),
                );
            }
            Role::User => {
                builder = builder.add_message(
                    mistralrs::TextMessageRole::User,
                    message.text.as_deref().unwrap_or_default(),
                );
            }
            Role::Assistant => {
                if message.tool_calls.is_empty() {
                    builder = builder.add_message(
                        mistralrs::TextMessageRole::Assistant,
                        message.text.as_deref().unwrap_or_default(),
                    );
                } else {
                    builder = builder.add_message_with_tool_call(
                        mistralrs::TextMessageRole::Assistant,
                        message.text.clone().unwrap_or_default(),
                        message
                            .tool_calls
                            .iter()
                            .map(to_tool_call_response)
                            .collect(),
                    );
                }
            }
            Role::Tool => {
                builder = builder.add_tool_message(
                    message.text.as_deref().unwrap_or_default(),
                    message.tool_call_id.as_deref().unwrap_or_default(),
                );
            }
        }
    }
    builder
}

fn to_tool_call_response(call: &ToolCall) -> mistralrs::ToolCallResponse {
    mistralrs::ToolCallResponse {
        index: 0,
        id: call.id.clone(),
        tp: mistralrs::ToolCallType::Function,
        function: mistralrs::CalledFunction {
            name: call.name.clone(),
            arguments: call.args.to_string(),
        },
    }
}

pub(crate) fn apply_sampling(
    builder: mistralrs::RequestBuilder,
    sampling: &SamplingParams,
) -> mistralrs::RequestBuilder {
    let builder = builder.set_sampler_max_len(sampling.max_tokens as usize);
    if is_deterministic(sampling) {
        builder.set_deterministic_sampler()
    } else {
        builder
            .set_sampler_temperature(f64::from(sampling.temperature))
            .set_sampler_topp(f64::from(sampling.top_p))
    }
}

/// Temperature 0.0 selects the deterministic sampler (lineage H22:
/// reproducible benchmark runs).
pub(crate) fn is_deterministic(sampling: &SamplingParams) -> bool {
    sampling.temperature == 0.0
}

/// Model output arguments arrive as a JSON string; tolerate non-JSON by
/// falling back to a bare string value (polymorphic args, Prion lesson #4).
pub(crate) fn parse_args(arguments: &str) -> serde_json::Value {
    serde_json::from_str(arguments)
        .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()))
}

/// Classify an engine error string: channel/timeout shapes are transient
/// (engine runs on its own thread and talks over channels), everything else
/// is permanent (C-007 disposition).
pub(crate) fn classify_anyhow(message: &str) -> ProviderError {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("channel") || lowered.contains("timeout") || lowered.contains("disconnect")
    {
        ProviderError::RetryableBackend(message.to_string())
    } else {
        ProviderError::Backend(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn constraint_maps_one_to_one() {
        let schema = json!({"type": "object"});
        assert!(matches!(
            map_constraint(&Constraint::JsonSchema(schema.clone())),
            mistralrs::Constraint::JsonSchema(s) if s == schema
        ));
        assert!(matches!(
            map_constraint(&Constraint::Regex("a+".to_string())),
            mistralrs::Constraint::Regex(r) if r == "a+"
        ));
        assert!(matches!(
            map_constraint(&Constraint::Lark("start: WORD".to_string())),
            mistralrs::Constraint::Lark(g) if g == "start: WORD"
        ));
    }

    #[test]
    fn tools_map_with_schema_reshape() {
        let tools = vec![ToolDescriptor {
            name: "write_file".to_string(),
            description: "write".to_string(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }];
        let mapped = map_tools(&tools);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].function.name, "write_file");
        let params = mapped[0].function.parameters.as_ref().unwrap();
        assert_eq!(params["type"], json!("object"));
        assert!(params.contains_key("properties"));
    }

    #[test]
    fn sampling_maps_with_deterministic_switch() {
        assert!(is_deterministic(&SamplingParams {
            temperature: 0.0,
            top_p: 0.95,
            max_tokens: 256,
        }));
        assert!(!is_deterministic(&SamplingParams::default()));
        // The builder calls themselves are exercised for both branches
        // (panic-free application is the contract; internals are opaque).
        let _ = apply_sampling(
            mistralrs::RequestBuilder::new(),
            &SamplingParams {
                temperature: 0.0,
                top_p: 0.95,
                max_tokens: 256,
            },
        );
        let _ = apply_sampling(mistralrs::RequestBuilder::new(), &SamplingParams::default());
    }

    #[test]
    fn args_parse_with_string_fallback() {
        assert_eq!(parse_args(r#"{"path": "a.txt"}"#), json!({"path": "a.txt"}));
        assert_eq!(parse_args("not json"), json!("not json"));
    }

    #[test]
    fn error_classification() {
        assert!(classify_anyhow("request channel disconnected").is_retryable());
        assert!(classify_anyhow("timeout waiting for engine").is_retryable());
        assert!(!classify_anyhow("GGUF magic mismatch").is_retryable());
    }

    #[test]
    fn message_mapping_is_panic_free_for_all_roles() {
        let messages = vec![
            Message::system("sys"),
            Message::user("hi"),
            Message {
                role: Role::Assistant,
                text: Some("calling".to_string()),
                tool_calls: vec![ToolCall {
                    id: "tc-1".to_string(),
                    name: "read_file".to_string(),
                    args: json!({"path": "x"}),
                }],
                tool_call_id: None,
            },
            Message::tool_result("tc-1", "contents"),
            Message::assistant("done"),
        ];
        let _ = map_messages(&messages);
    }
}
