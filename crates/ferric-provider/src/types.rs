use serde::{Deserialize, Serialize};
use thiserror::Error;

use ferric_core::Message;

/// A decoding constraint, shaped after llguidance's three grammar kinds so
/// both the in-process backend (mistral.rs, llguidance merged) and the HTTP
/// escape valve (llama-server json_schema/GBNF) map onto it without trait
/// changes (ADR-003).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Constraint {
    JsonSchema(serde_json::Value),
    Regex(String),
    Lark(String),
}

/// What a backend can actually do. The loop downgrades the action protocol
/// per these flags (e.g. no constraint support → fenced-code protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub supports_constraint: bool,
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
    pub constraint: Option<Constraint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub message: Message,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("mock script exhausted after {0} completions")]
    ScriptExhausted(usize),

    #[error("backend error: {0}")]
    Backend(String),

    #[error("request invalid: {0}")]
    InvalidRequest(String),
}
