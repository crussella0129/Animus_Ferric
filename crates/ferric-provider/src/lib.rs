//! Async, constraint-carrying `Provider` trait + the deterministic MockProvider.
//!
//! Two real backends:
//! - the OpenAI-compatible HTTP valve (feature `backend-openai`) — enforces a
//!   harness-authored JSON-Schema constraint server-side via `response_format`,
//!   where "the harness owns decoding" is actually true (ADR-022).
//!
//! The embedded PyO3/PyTorch backend was removed (ADR-021): external engines are
//! reached only via the out-of-process valve. Per ADR-009, any change touching
//! this crate requires a real-model run before merge.

mod mock;
#[cfg(feature = "backend-openai")]
pub mod openai;
mod stream_scan;
mod traits;
mod types;

pub use mock::MockProvider;
#[cfg(feature = "backend-openai")]
pub use openai::{OpenAiConfig, OpenAiProvider};
pub use stream_scan::ConstrainedJsonScanner;
pub use traits::Provider;
pub use types::{
    Capabilities, Completion, CompletionRequest, Constraint, ProviderError, SamplingParams,
    StreamDelta, ToolDescriptor,
};
