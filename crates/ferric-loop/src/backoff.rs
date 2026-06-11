//! Exponential backoff on retryable provider errors (Prion failure-mode #6).
//!
//! Base 250 ms, doubling, max 3 retries. Non-retryable errors propagate
//! immediately with zero sleeps. Sleeping goes through the injectable
//! `Sleeper` so tests record the schedule instead of waiting.

use std::time::Duration;

use ferric_provider::{Completion, CompletionRequest, Provider, ProviderError};

use crate::run::Sleeper;

pub const BASE_DELAY_MS: u64 = 250;
pub const MAX_RETRIES: u32 = 3;

pub async fn complete_with_backoff(
    provider: &dyn Provider,
    request: CompletionRequest,
    sleeper: &dyn Sleeper,
) -> Result<Completion, ProviderError> {
    let mut attempt = 0u32;
    loop {
        match provider.complete(request.clone()).await {
            Ok(completion) => return Ok(completion),
            Err(e) if e.is_retryable() && attempt < MAX_RETRIES => {
                let delay = BASE_DELAY_MS << attempt; // 250, 500, 1000
                sleeper.sleep(Duration::from_millis(delay));
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}
