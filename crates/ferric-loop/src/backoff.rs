//! Exponential backoff on retryable provider errors (Prion failure-mode #6).
//!
//! Base 250 ms, doubling, max 3 retries. Non-retryable errors propagate
//! immediately with zero sleeps. Sleeping goes through the injectable
//! `Sleeper` so tests record the schedule instead of waiting.

use std::time::Duration;

use ferric_provider::{Completion, CompletionRequest, Provider, ProviderError, StreamDelta};
use tracing::warn;

use crate::run::Sleeper;

pub const BASE_DELAY_MS: u64 = 250;
pub const MAX_RETRIES: u32 = 3;

/// Complete `request`, retrying retryable errors. With `on_delta` set, each
/// attempt streams (ADR-047): a retryable error mid-stream retries the WHOLE
/// request fresh — each attempt gets its own independent set of deltas (a
/// provider's `complete_streaming` starts from a clean accumulator every call),
/// so a failed attempt's already-fired deltas are never replayed or duplicated
/// by the next attempt.
pub async fn complete_with_backoff(
    provider: &dyn Provider,
    request: CompletionRequest,
    sleeper: &dyn Sleeper,
    on_delta: Option<&(dyn Fn(StreamDelta) + Sync)>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Result<Completion, ProviderError> {
    let mut attempt = 0u32;
    loop {
        let result = match on_delta {
            Some(on_delta) => {
                provider
                    .complete_streaming(request.clone(), on_delta, cancel_flag.clone())
                    .await
            }
            None => {
                provider
                    .complete(request.clone(), cancel_flag.clone())
                    .await
            }
        };
        match result {
            Ok(completion) => return Ok(completion),
            Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                let delay = BASE_DELAY_MS << attempt; // 250, 500, 1000
                let message = if on_delta.is_some() {
                    "retryable provider error mid-stream; backing off (full retry)"
                } else {
                    "retryable provider error; backing off"
                };
                warn!(
                    error = %e,
                    attempt = attempt + 1,
                    max_retries = MAX_RETRIES,
                    delay_ms = delay,
                    "{message}"
                );
                sleeper.sleep(Duration::from_millis(delay));
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}
