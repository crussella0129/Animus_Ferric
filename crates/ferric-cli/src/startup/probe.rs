use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{StartupError, check_cancelled};

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;

pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const BODY_LIMIT: usize = 1024 * 1024;
pub(super) const MODEL_LIMIT: usize = 128;

pub(super) fn endpoint(value: &str) -> Result<String, StartupError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| StartupError::resource("The configured endpoint is invalid."))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(StartupError::resource(
            "Use an HTTP(S) endpoint without embedded credentials or query parameters.",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub(super) fn models(
    base: &str,
    key: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<String>, StartupError> {
    let body = get(&format!("{base}/models"), key, cancel, deadline, true)?
        .ok_or_else(|| StartupError::resource("The server did not return model metadata."))?;
    parse_models(&body)
}

pub(super) fn health(
    base: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<bool, StartupError> {
    let root = base.strip_suffix("/v1").unwrap_or(base);
    Ok(get(
        &format!("{root}/health"),
        "ferric-local",
        cancel,
        deadline,
        false,
    )?
    .is_some())
}

fn get(
    url: &str,
    key: &str,
    cancel: &AtomicBool,
    deadline: Instant,
    metadata: bool,
) -> Result<Option<Vec<u8>>, StartupError> {
    check_cancelled(cancel)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(StartupError::resource(
            "Startup reached its 180-second deadline.",
        ));
    }
    let timeout = remaining.min(PROBE_TIMEOUT);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| StartupError::resource("The network runtime could not start."))?;
    runtime.block_on(cancellable(cancel, async {
        let builder = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none());
        // Listener ownership authorizes only a direct loopback exchange. An
        // ambient HTTP proxy must not substitute unrelated model metadata.
        let builder = if reqwest::Url::parse(url)
            .ok()
            .is_some_and(|url| matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
        {
            builder.no_proxy()
        } else {
            builder
        };
        let client = builder
            .build()
            .map_err(|_| StartupError::resource("The model probe could not be prepared."))?;
        let mut response = client.get(url).bearer_auth(key).send().await.map_err(|_| {
            StartupError::resource("The server probe failed or exceeded five seconds.")
        })?;
        if !response.status().is_success() {
            if !metadata && response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                return Ok(None);
            }
            return Err(StartupError::resource(
                "The server rejected the probe or redirected it.",
            ));
        }
        if !metadata {
            return Ok(Some(Vec::new()));
        }
        if response
            .content_length()
            .is_some_and(|size| size > BODY_LIMIT as u64)
        {
            return Err(StartupError::resource(
                "Server model metadata exceeds one MiB.",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            StartupError::resource("Server model metadata was incomplete or timed out.")
        })? {
            if chunk.len() > BODY_LIMIT.saturating_sub(bytes.len()) {
                return Err(StartupError::resource(
                    "Server model metadata exceeds one MiB.",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Some(bytes))
    }))
}

async fn cancellable<T>(
    cancel: &AtomicBool,
    operation: impl Future<Output = Result<T, StartupError>>,
) -> Result<T, StartupError> {
    tokio::pin!(operation);
    let mut tick = tokio::time::interval(Duration::from_millis(25));
    loop {
        tokio::select! {
            biased;
            _ = tick.tick() => check_cancelled(cancel)?,
            result = &mut operation => {
                if cancel.load(Ordering::Relaxed) { return Err(StartupError::cancelled()); }
                return result;
            }
        }
    }
}

pub(super) fn parse_models(bytes: &[u8]) -> Result<Vec<String>, StartupError> {
    if bytes.len() > BODY_LIMIT {
        return Err(StartupError::resource(
            "Server model metadata exceeds one MiB.",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| StartupError::resource("Server model metadata is not valid JSON."))?;
    let records = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| StartupError::resource("The server did not provide a model list."))?;
    if records.is_empty() || records.len() > MODEL_LIMIT {
        return Err(StartupError::resource(
            "The server model list is empty or exceeds 128 entries.",
        ));
    }
    let mut ids = BTreeSet::new();
    for record in records {
        let id = record
            .get("id")
            .or_else(|| record.get("model"))
            .or_else(|| record.get("name"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StartupError::resource("A server model identifier is missing."))?;
        if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
            return Err(StartupError::resource(
                "A server model identifier is invalid or too long.",
            ));
        }
        ids.insert(id.to_string());
    }
    Ok(ids.into_iter().collect())
}
