use serde::{Deserialize, Serialize};
use thiserror::Error;

use ferric_core::Message;

/// What a backend can actually do. The loop selects the action protocol per
/// these flags: `supports_constraint` → `ConstrainedJson`, else
/// `supports_native_tool_calls` → `NativeTools`, else `TextXml` (ADR-015/022).
///
/// These flags are a STRUCTURAL CONTRACT, not aspiration: a backend may only
/// advertise a capability its `complete()` actually exercises on the path that
/// runs. (Backends that report `supports_native_tool_calls: true` while
/// silently stripping tools trigger ADR-022's structural-honesty ruling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub supports_native_tool_calls: bool,
    /// The backend enforces a harness-authored `Constraint` server-side
    /// (e.g. the HTTP valve's `response_format`). When true the loop may run
    /// `ConstrainedJson` — the founding "harness owns decoding" thesis.
    pub supports_constraint: bool,
    pub exposes_logits: bool,
    /// The backend can carry non-text content parts (image/audio/video) to the
    /// model (ADR-023). `true` for the OpenAI valve (it forwards an OpenAI
    /// content-parts array); `false` for the text-only in-process path. Honest
    /// per ADR-022 — gating refuses to send media a backend cannot carry.
    pub supports_media: bool,
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

/// A harness-authored decoding constraint (ADR-003, llguidance-shaped). The
/// loop authors it; a constraint-honoring backend enforces it server-side so a
/// malformed action is unrepresentable rather than repairable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Constraint {
    /// A JSON Schema the whole completion must satisfy (the unified action
    /// grammar). Sent to the HTTP valve as `response_format.json_schema`.
    JsonSchema(serde_json::Value),
    /// A regular expression the whole completion must match.
    Regex(String),
    /// A Lark/GBNF context-free grammar.
    Lark(String),
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

/// An incremental piece of a streaming completion, pushed to a caller-supplied
/// sink as it becomes available (ADR-047). `Text` is human-readable prose worth
/// displaying live; `ToolNamed` is a cheap early "which tool is being called"
/// activity signal, available before the tool's `args` finish streaming.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamDelta {
    Text(String),
    ToolNamed(String),
    Thought(String),
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
    fn constraint_jsonschema_serde_roundtrip() {
        let c = Constraint::JsonSchema(json!({"type": "object", "required": ["x"]}));
        let s = serde_json::to_string(&c).unwrap();
        let back: Constraint = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn retryability_per_variant() {
        assert!(ProviderError::RetryableBackend("timeout".to_string()).is_retryable());
        assert!(!ProviderError::Backend("bad gguf".to_string()).is_retryable());
        assert!(!ProviderError::ScriptExhausted(2).is_retryable());
        assert!(!ProviderError::InvalidRequest("x".to_string()).is_retryable());
    }
}
