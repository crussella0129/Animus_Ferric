use async_trait::async_trait;

use crate::types::{Capabilities, Completion, CompletionRequest, ProviderError, StreamDelta};

/// An inference backend.
///
/// Async and dyn-compatible from day one (ADR-003): the s1 backends are
/// tokio-async (mistral.rs) and reqwest-async (OpenAI-compatible HTTP), and a
/// sync trait here would force a breaking redesign.
///
/// Backends own their heavy state (loaded model, HTTP pool) internally; the
/// trait itself is stateless per request.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable identifier, e.g. `"mock"`, `"mistralrs"`, `"openai-http"`.
    fn id(&self) -> &str;

    fn capabilities(&self) -> Capabilities;

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ProviderError>;

    /// Streaming variant (ADR-047, filling ADR-003's reserved extension
    /// point): identical contract to `complete()` — same return type, same
    /// error semantics — but pushes `StreamDelta`s to `on_delta` as they
    /// become available. The DEFAULT implementation calls `complete()` and
    /// fires at most one `Text` delta with the full completion text once it's
    /// in (nothing for a pure tool-calls completion with no text) — every
    /// provider that doesn't override this behaves identically to `complete()`
    /// with zero code and zero behavior change. Only a backend with a real
    /// token stream (the OpenAI-compatible HTTP valve) overrides it.
    ///
    /// `on_delta` takes `StreamDelta` by value (not `&StreamDelta`) — cheap to
    /// move (one `String`), and sidesteps an `async_trait` lifetime-elision
    /// limitation with `dyn Fn(&T)` parameters in a default async method body.
    /// `&dyn Fn` rather than `FnMut` keeps this dyn-compatible with no
    /// interior-mutability requirement on the trait itself; a caller needing
    /// to accumulate state across deltas captures a `Mutex`/`Cell` (`Sync` is
    /// required — `async_trait`'s generated future must be `Send`, so a
    /// captured `&dyn Fn` reference held across an `.await` needs its
    /// referent to be `Sync`).
    async fn complete_streaming(
        &self,
        request: CompletionRequest,
        on_delta: &(dyn Fn(StreamDelta) + Sync),
    ) -> Result<Completion, ProviderError> {
        let completion = self.complete(request).await?;
        if let Some(text) = &completion.message.text {
            on_delta(StreamDelta::Text(text.clone()));
        }
        Ok(completion)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ferric_core::{Message, Role, ToolCall};

    use crate::mock::MockProvider;
    use crate::types::{Completion, SamplingParams};

    use super::*;

    fn request() -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message::user("hi")],
            sampling: SamplingParams::default(),
            tools: Vec::new(),
            constraint: None,
        }
    }

    fn text_completion(text: &str) -> Completion {
        Completion {
            message: Message::assistant(text),
            input_tokens: Some(10),
            output_tokens: Some(5),
            truncated: false,
        }
    }

    fn tool_call_completion() -> Completion {
        Completion {
            message: Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "0".to_string(),
                    name: "write_file".to_string(),
                    args: serde_json::json!({"path": "a.txt", "content": "x"}),
                }],
                tool_call_id: None,
                media: Vec::new(),
            },
            input_tokens: Some(10),
            output_tokens: Some(5),
            truncated: false,
        }
    }

    /// T-3701: the default `complete_streaming` (no override, e.g. `MockProvider`)
    /// returns the same `Completion` `complete()` would and fires exactly one
    /// `Text` delta for a text-bearing completion.
    #[test]
    fn default_complete_streaming_matches_complete_with_text() {
        let provider = MockProvider::new(vec![text_completion("hello there")]);
        let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
        let sink = |d: StreamDelta| deltas.lock().unwrap().push(d);

        let streamed =
            futures_executor::block_on(provider.complete_streaming(request(), &sink)).unwrap();
        let plain = futures_executor::block_on(
            MockProvider::new(vec![text_completion("hello there")]).complete(request()),
        )
        .unwrap();

        assert_eq!(streamed, plain);
        assert_eq!(
            deltas.into_inner().unwrap(),
            vec![StreamDelta::Text("hello there".to_string())]
        );
    }

    /// T-3701: a tool-calls-only completion (no text) fires zero deltas —
    /// there's no prose to show, and (test-critique C-007) the default impl
    /// never fires `ToolNamed` either, since that's a real-streaming-only
    /// signal only `OpenAiProvider`'s actual implementation produces.
    #[test]
    fn default_complete_streaming_no_delta_for_tool_only() {
        let provider = MockProvider::new(vec![tool_call_completion()]);
        let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
        let sink = |d: StreamDelta| deltas.lock().unwrap().push(d);

        futures_executor::block_on(provider.complete_streaming(request(), &sink)).unwrap();

        assert!(deltas.into_inner().unwrap().is_empty());
    }

    /// Same as above, phrased as the explicit C-007 invariant: not just "no
    /// Text", but specifically no `ToolNamed` either.
    #[test]
    fn default_complete_streaming_never_fires_tool_named() {
        let provider = MockProvider::new(vec![tool_call_completion()]);
        let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
        let sink = |d: StreamDelta| deltas.lock().unwrap().push(d);

        futures_executor::block_on(provider.complete_streaming(request(), &sink)).unwrap();

        assert!(
            !deltas
                .into_inner()
                .unwrap()
                .iter()
                .any(|d| matches!(d, StreamDelta::ToolNamed(_)))
        );
    }
}
