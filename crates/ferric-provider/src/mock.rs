use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::traits::Provider;
use crate::types::{Capabilities, Completion, CompletionRequest, ProviderError};

/// Deterministic scripted provider for tests and dry runs: replays a fixed
/// sequence of completions and records every request it receives, so tests
/// can assert exactly what the harness sent (including constraints).
pub struct MockProvider {
    state: Mutex<MockState>,
}

struct MockState {
    script: VecDeque<Completion>,
    served: usize,
    requests: Vec<CompletionRequest>,
}

impl MockProvider {
    pub fn new(script: Vec<Completion>) -> Self {
        Self {
            state: Mutex::new(MockState {
                script: script.into(),
                served: 0,
                requests: Vec::new(),
            }),
        }
    }

    /// The most recent request received, if any.
    pub fn last_request(&self) -> Option<CompletionRequest> {
        self.state.lock().unwrap().requests.last().cloned()
    }

    /// All requests received, in order.
    pub fn requests(&self) -> Vec<CompletionRequest> {
        self.state.lock().unwrap().requests.clone()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: false,
            exposes_logits: false,
            supports_media: false,
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ProviderError> {
        let mut state = self.state.lock().unwrap();
        state.requests.push(request);
        match state.script.pop_front() {
            Some(completion) => {
                state.served += 1;
                Ok(completion)
            }
            None => Err(ProviderError::ScriptExhausted(state.served)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::Message;

    use crate::types::SamplingParams;

    fn completion(text: &str) -> Completion {
        Completion {
            message: Message::assistant(text),
            input_tokens: Some(10),
            output_tokens: Some(5),
            truncated: false,
        }
    }

    fn request() -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message::user("hi")],
            sampling: SamplingParams::default(),
            tools: Vec::new(),
            constraint: None,
        }
    }

    #[test]
    fn mock_replays_script_in_order() {
        let mock = MockProvider::new(vec![completion("a"), completion("b")]);
        let a = futures_executor::block_on(mock.complete(request())).unwrap();
        let b = futures_executor::block_on(mock.complete(request())).unwrap();
        assert_eq!(a.message.text.as_deref(), Some("a"));
        assert_eq!(b.message.text.as_deref(), Some("b"));
        let err = futures_executor::block_on(mock.complete(request())).unwrap_err();
        assert!(matches!(err, ProviderError::ScriptExhausted(2)));
    }

    #[test]
    fn provider_is_dyn_compatible() {
        let boxed: Box<dyn Provider> = Box::new(MockProvider::new(vec![completion("dyn")]));
        assert_eq!(boxed.id(), "mock");
        let done = futures_executor::block_on(boxed.complete(request())).unwrap();
        assert_eq!(done.message.text.as_deref(), Some("dyn"));
    }
}
