use serde::{Deserialize, Serialize};
use thiserror::Error;

use ferric_core::Message;



/// What a backend can actually do. The loop downgrades the action protocol
/// per these flags (e.g. no constraint support → fenced-code protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub supports_native_tool_calls: bool,
    pub exposes_logits: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.95,
            max_tokens: 2048,
        }
    }
}

/// What the model is told a tool looks like. Deliberately independent of
/// `ferric-tools` (which sits above this crate in the dependency order): the
/// registry converts its richer `ToolSpec` down to this wire shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub sampling: SamplingParams,
    pub tools: Vec<ToolDescriptor>,
}

impl CompletionRequest {
    pub fn validate(&self) -> Result<(), ProviderError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub message: Message,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    /// True when generation hit the token limit (`finish_reason == "length"`).
    /// Under a grammar this is the one malformed-action case the constraint
    /// cannot prevent (ADR-015): the loop must not parse a truncated action.
    pub truncated: bool,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("mock script exhausted after {0} completions")]
    ScriptExhausted(usize),

    /// Permanent backend failure (model load, GGUF parse, template errors).
    #[error("backend error: {0}")]
    Backend(String),

    /// Transient backend failure (timeouts, channel disconnects) — the loop
    /// retries these with exponential backoff.
    #[error("retryable backend error: {0}")]
    RetryableBackend(String),

    #[error("request invalid: {0}")]
    InvalidRequest(String),
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, ProviderError::RetryableBackend(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::Message;
    use serde_json::json;

    fn request(with_tool: bool) -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message::user("hi")],
            sampling: SamplingParams::default(),
            tools: if with_tool {
                vec![ToolDescriptor {
                    name: "t".to_string(),
                    description: "d".to_string(),
                    input_schema: json!({"type": "object"}),
                }]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn validate_matrix() {
        assert!(request(false).validate().is_ok());
        assert!(request(true).validate().is_ok());
    }

    #[test]
    fn retryability_per_variant() {
        assert!(ProviderError::RetryableBackend("timeout".to_string()).is_retryable());
        assert!(!ProviderError::Backend("bad gguf".to_string()).is_retryable());
        assert!(!ProviderError::ScriptExhausted(2).is_retryable());
        assert!(!ProviderError::InvalidRequest("x".to_string()).is_retryable());
    }
}
