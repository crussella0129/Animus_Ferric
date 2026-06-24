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

use std::path::PathBuf;

use async_trait::async_trait;

use ferric_core::{Message, Role, ToolCall};

use crate::traits::Provider;
use crate::types::{Capabilities, Completion, CompletionRequest, ProviderError, SamplingParams};

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
    /// Path to the model's real `tokenizer.json`. REQUIRED FOR GRAMMAR
    /// (ADR-020 root cause): without it, mistral.rs synthesizes a tokenizer
    /// from GGUF metadata whose byte-level vocab yields a malformed llguidance
    /// toktrie, hanging any `Constraint::JsonSchema`. Supplying the authentic
    /// tokenizer.json takes the loader's real-tokenizer branch and fixes it.
    pub tokenizer_json: Option<PathBuf>,
    /// Alternatively, an HF model id to source tokenizer.json from (used only
    /// when `tokenizer_json` is None). Requires network unless cached.
    pub tok_model_id: Option<String>,
}

impl MistralRsConfig {
    pub fn new(model_dir: impl Into<PathBuf>, model_file: impl Into<String>) -> Self {
        Self {
            model_dir: model_dir.into(),
            model_file: model_file.into(),
            chat_template: None,
            max_num_seqs: 2,
            force_cpu: true,
            tokenizer_json: None,
            tok_model_id: None,
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
        let model = if config.model_file.ends_with(".gguf") {
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
            if let Some(tok_json) = &config.tokenizer_json {
                builder = builder.with_tokenizer_json(tok_json.display().to_string());
            } else if let Some(tok_id) = &config.tok_model_id {
                builder = builder.with_tok_model_id(tok_id.clone());
            }
            builder
                .build()
                .await
                .map_err(|e| ProviderError::Backend(format!("model load: {e:#}")))?
        } else {
            let full_path = config.model_dir.join(&config.model_file);
            let mut builder = mistralrs::TextModelBuilder::new(full_path.display().to_string())
                .with_max_num_seqs(config.max_num_seqs);
            if config.force_cpu {
                builder = builder.with_force_cpu();
            }
            if let Some(template) = &config.chat_template {
                builder = builder.with_chat_template(template);
            }

            builder
                .build()
                .await
                .map_err(|e| ProviderError::Backend(format!("model load: {e:#}")))?
        };
        Ok(Self { model })
    }
}

#[async_trait]
impl Provider for MistralRsProvider {
    fn id(&self) -> &str {
        "mistralrs"
    }

    fn capabilities(&self) -> Capabilities {
        // Honest (ADR-022): `complete()` passes NEITHER tools NOR a grammar to
        // the engine — tools are stripped (the s3 pivot) and a JSON-Schema
        // constraint hangs llguidance on GGUF (ADR-020). So this backend does
        // not do native tool calls and does not enforce constraints; the loop
        // routes it to `TextXml` (the model emits `<tool_call>` XML, scraped by
        // the loop). Reporting `true` here was the s6 toolbench 0.0% bug.
        Capabilities {
            supports_native_tool_calls: false,
            supports_constraint: false,
            exposes_logits: false,
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ProviderError> {
        // ADR-010 defense in depth: reject before contacting the engine.
        request.validate()?;

        let mut builder = map_messages(&request.messages);
        builder = apply_sampling(builder, &request.sampling);
        // No engine-level tools or grammar constraints are passed (s3 pivot).

        let response = match tokio::time::timeout(
            std::time::Duration::from_secs(300),
            self.model.send_chat_request(builder),
        )
        .await
        {
            Ok(res) => match res {
                Ok(r) => r,
                Err(e) => return Err(classify_anyhow(&format!("{e:#}"))),
            },
            Err(_) => {
                return Err(ProviderError::Backend(
                    "inference timeout: engine hung for >5m".to_string(),
                ));
            }
        };

        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::Backend("response carried no choices".to_string()))?;
        let finish_reason = choice.finish_reason.clone();

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
                media: Vec::new(),
            },
            input_tokens: Some(response.usage.prompt_tokens as u32),
            output_tokens: Some(response.usage.completion_tokens as u32),
            truncated: is_truncated(&finish_reason),
        })
    }
}

/// `finish_reason` values per mistralrs-core 0.8.1 sequence.rs Display impl:
/// "stop" | "length" | "canceled" | "tool_calls" | ... Only "length" means
/// the output was cut by the token budget.
pub(crate) fn is_truncated(finish_reason: &str) -> bool {
    finish_reason == "length"
}

// ---- mapping layer: free functions so they unit-test without a model ----

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
    fn finish_reason_maps_truncated() {
        assert!(is_truncated("length"));
        assert!(!is_truncated("stop"));
        assert!(!is_truncated("tool_calls"));
        assert!(!is_truncated("canceled"));
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
                media: Vec::new(),
            },
            Message::tool_result("tc-1", "contents"),
            Message::assistant("done"),
        ];
        let _ = map_messages(&messages);
    }
}
