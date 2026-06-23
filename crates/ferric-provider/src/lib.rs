//! Async Provider trait with constraint plumbing, plus the deterministic MockProvider.
//!
//! Real backends land in s1: mistral.rs in-process (flagship pure-Rust path)
//! and an OpenAI-compatible HTTP client (escape valve). Per the lineage's
//! real-GGUF validation policy (ADR-009), any change touching this crate from
//! s1 onward requires a real-model run before merge.

#[cfg(feature = "backend-mistralrs")]
pub mod mistralrs;
mod mock;
#[cfg(feature = "backend-openai")]
pub mod openai;
mod traits;
mod types;

pub use mock::MockProvider;
#[cfg(feature = "backend-openai")]
pub use openai::{OpenAiConfig, OpenAiProvider};
pub use traits::Provider;
pub use types::{
    Capabilities, Completion, CompletionRequest, Constraint, ProviderError, SamplingParams,
    ToolDescriptor,
};
