use serde::{Deserialize, Serialize};
use thiserror::Error;

use ferric_core::Message;

// Moved to the constrained-decoding core (T-12601, INT-0011); re-exported so
// every `ferric_provider::…` path is unchanged.
pub use ferric_iron::types::{Capabilities, Constraint, StreamDelta, ToolDescriptor};

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

#[derive(Debug, Clone, PartialEq)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub sampling: SamplingParams,
    pub tools: Vec<ToolDescriptor>,
    /// A decoding constraint over the whole output. Mutually exclusive with
    /// `tools` (ADR-010): a constraint applies to the ENTIRE completion and
    /// fights tool-call syntax. `validate()` rejects both-set requests.
    pub constraint: Option<Constraint>,
}

impl CompletionRequest {
    /// ADR-010: a constraint and native tool calling are mutually exclusive
    /// per request. The loop validates before every provider call (primary);
    /// backends validate again at their boundary (defense in depth).
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.constraint.is_some() && !self.tools.is_empty() {
            return Err(ProviderError::InvalidRequest(
                "constraint and tools are mutually exclusive (ADR-010): a \
                 constraint governs the whole output and fights tool-call syntax"
                    .to_string(),
            ));
        }
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

    fn request(with_tool: bool, with_constraint: bool) -> CompletionRequest {
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
            constraint: if with_constraint {
                Some(Constraint::JsonSchema(json!({"type": "object"})))
            } else {
                None
            },
        }
    }

    #[test]
    fn validate_rejects_constraint_and_tools() {
        // ADR-010: constraint + tools in the same request is invalid.
        assert!(matches!(
            request(true, true).validate(),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[test]
    fn validate_accepts_lawful_combinations() {
        assert!(request(false, true).validate().is_ok()); // constraint only
        assert!(request(true, false).validate().is_ok()); // tools only
        assert!(request(false, false).validate().is_ok()); // neither
    }

    #[test]
    fn retryability_per_variant() {
        assert!(ProviderError::RetryableBackend("timeout".to_string()).is_retryable());
        assert!(!ProviderError::Backend("bad gguf".to_string()).is_retryable());
        assert!(!ProviderError::ScriptExhausted(2).is_retryable());
        assert!(!ProviderError::InvalidRequest("x".to_string()).is_retryable());
    }
}
