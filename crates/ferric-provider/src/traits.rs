use async_trait::async_trait;

use crate::types::{Capabilities, Completion, CompletionRequest, ProviderError};

/// An inference backend.
///
/// Async and dyn-compatible from day one (ADR-003): the s1 backends are
/// tokio-async (mistral.rs) and reqwest-async (OpenAI-compatible HTTP), and a
/// sync trait here would force a breaking redesign. Streaming is a reserved
/// extension point (`complete_stream` yielding `ProviderEvent`s), added in s1
/// when real token streams exist.
///
/// Backends own their heavy state (loaded model, HTTP pool) internally; the
/// trait itself is stateless per request.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable identifier, e.g. `"mock"`, `"mistralrs"`, `"openai-http"`.
    fn id(&self) -> &str;

    fn capabilities(&self) -> Capabilities;

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ProviderError>;
}
