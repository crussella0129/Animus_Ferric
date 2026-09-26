//! The constrained exchange with the upstream backend (T-12606, INT-0012
//! AC-1/2/3).
//!
//! The upstream request always streams: that is what makes cancellation
//! prompt and liveness honest. The exchange watches for the client going away
//! throughout. Dropping the upstream response closes its connection, which is
//! how llama.cpp learns to stop generating.
//!
//! Failures are explicit:
//! - before any byte reaches the client, the server answers HTTP 502;
//! - after streaming has started, the stream ends with an in-band OpenAI
//!   `error` event.
//!
//! The valve never returns unconstrained text as an answer.

use std::future::Future;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::sse::{ChunkIdentity, SseLine, SseLineBuffer, chunk, classify, done_frame, frame};
use crate::translate::{ActionAssembler, ActionError, Resolution};

/// Bytes of an upstream error body quoted back to the client.
const ERROR_EXCERPT_BYTES: usize = 2_048;

/// Why an exchange failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValveError {
    #[error("upstream unreachable: {0}")]
    UpstreamUnreachable(String),
    #[error("upstream returned HTTP {status}: {excerpt}")]
    UpstreamHttp { status: u16, excerpt: String },
    #[error("upstream stream failed: {0}")]
    UpstreamStream(String),
    #[error(transparent)]
    Action(#[from] ActionError),
}

impl ValveError {
    /// Stable receipt label.
    pub fn class(&self) -> &'static str {
        match self {
            ValveError::UpstreamUnreachable(_) => "upstream_unreachable",
            ValveError::UpstreamHttp { .. } => "upstream_http",
            ValveError::UpstreamStream(_) => "upstream_stream",
            ValveError::Action(error) => error.class(),
        }
    }

    /// The OpenAI-shaped error object, used as a 502 body or an in-band event.
    pub fn body(&self) -> Value {
        let mut error = json!({
            "message": self.to_string(),
            "type": "ferric_valve_error",
            "code": self.class(),
        });
        if let ValveError::UpstreamHttp { status, .. } = self {
            error["upstream_status"] = json!(status);
        }
        json!({ "error": error })
    }
}

/// Upstream-reported identity and cost, as far as the backend reported it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpstreamStats {
    pub model: Option<String>,
    /// Total prompt tokens (cached and evaluated).
    pub prompt_tokens: Option<u64>,
    /// Prompt tokens actually evaluated this request (llama.cpp `timings.prompt_n`).
    pub prompt_eval_tokens: Option<u64>,
    /// Prompt tokens served from the backend's cache.
    pub cached_tokens: Option<u64>,
    pub predicted_tokens: Option<u64>,
    pub prompt_ms: Option<f64>,
    pub predicted_ms: Option<f64>,
}

impl UpstreamStats {
    /// Fold in whatever one response object or chunk reports.
    pub fn absorb(&mut self, value: &Value) {
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            self.model = Some(model.to_string());
        }
        if let Some(usage) = value.get("usage").filter(|usage| usage.is_object()) {
            if let Some(n) = usage.get("prompt_tokens").and_then(Value::as_u64) {
                self.prompt_tokens = Some(n);
            }
            if let Some(n) = usage.get("completion_tokens").and_then(Value::as_u64) {
                self.predicted_tokens = Some(n);
            }
            if let Some(n) = usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(Value::as_u64)
            {
                self.cached_tokens = Some(n);
            }
        }
        if let Some(timings) = value.get("timings").filter(|timings| timings.is_object()) {
            let int = |key: &str| timings.get(key).and_then(Value::as_u64);
            let float = |key: &str| timings.get(key).and_then(Value::as_f64);
            if let Some(n) = int("prompt_n") {
                self.prompt_eval_tokens = Some(n);
            }
            if let Some(n) = int("cache_n") {
                self.cached_tokens = Some(n);
            }
            if let Some(n) = int("predicted_n") {
                self.predicted_tokens = Some(n);
            }
            if let Some(ms) = float("prompt_ms") {
                self.prompt_ms = Some(ms);
            }
            if let Some(ms) = float("predicted_ms") {
                self.predicted_ms = Some(ms);
            }
            if self.prompt_tokens.is_none()
                && let (Some(evaluated), Some(cached)) =
                    (self.prompt_eval_tokens, self.cached_tokens)
            {
                self.prompt_tokens = Some(evaluated + cached);
            }
        }
    }

    /// Names of the metrics the upstream did not report.
    pub fn unavailable(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.model.is_none() {
            missing.push("model");
        }
        if self.prompt_tokens.is_none() {
            missing.push("prompt_tokens");
        }
        if self.prompt_eval_tokens.is_none() {
            missing.push("prompt_eval_tokens");
        }
        if self.cached_tokens.is_none() {
            missing.push("cached_tokens");
        }
        if self.predicted_tokens.is_none() {
            missing.push("predicted_tokens");
        }
        if self.prompt_ms.is_none() {
            missing.push("prompt_ms");
        }
        if self.predicted_ms.is_none() {
            missing.push("predicted_ms");
        }
        missing
    }
}

/// How an exchange ended.
#[derive(Debug, Default)]
pub struct ExchangeOutcome {
    pub resolution: Option<Resolution>,
    pub error: Option<ValveError>,
    pub cancelled: bool,
    pub stats: UpstreamStats,
}

/// One constrained request to the upstream.
pub struct Exchange<'a> {
    pub client: &'a reqwest::Client,
    /// Full upstream URL, e.g. `http://127.0.0.1:8080/v1/chat/completions`.
    pub url: String,
    pub authorization: Option<String>,
    pub body: &'a Value,
    pub admitted: Vec<String>,
    pub call_id: String,
    pub identity: ChunkIdentity,
    pub heartbeat: Duration,
}

/// Run a constrained exchange.
///
/// - `frames`: `Some` for a streaming client; SSE frames are sent there.
/// - `started`: resolved `Ok` once the upstream answered 2xx, or `Err` if the
///   exchange failed before any byte streamed, so the server can still choose
///   the HTTP status.
/// - `closed`: resolves when the client has gone away.
pub async fn run_constrained<F>(
    exchange: Exchange<'_>,
    frames: Option<mpsc::Sender<String>>,
    started: Option<oneshot::Sender<Result<(), ValveError>>>,
    closed: F,
) -> ExchangeOutcome
where
    F: Future<Output = ()>,
{
    let mut outcome = ExchangeOutcome::default();
    tokio::pin!(closed);

    let mut request = exchange.client.post(&exchange.url).json(exchange.body);
    if let Some(authorization) = &exchange.authorization {
        request = request.header("Authorization", authorization);
    }
    let sent = tokio::select! {
        biased;
        () = &mut closed => {
            outcome.cancelled = true;
            return outcome;
        }
        sent = request.send() => sent,
    };
    let mut response = match sent {
        Ok(response) => response,
        Err(error) => {
            return fail_before_start(
                outcome,
                started,
                ValveError::UpstreamUnreachable(error.to_string()),
            );
        }
    };
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let excerpt = tokio::select! {
            biased;
            () = &mut closed => {
                outcome.cancelled = true;
                return outcome;
            }
            text = response.text() => text.unwrap_or_default(),
        };
        let excerpt: String = excerpt.chars().take(ERROR_EXCERPT_BYTES).collect();
        return fail_before_start(
            outcome,
            started,
            ValveError::UpstreamHttp { status, excerpt },
        );
    }
    if let Some(started) = started {
        let _ = started.send(Ok(()));
    }

    let identity = exchange.identity;
    let mut assembler = ActionAssembler::new(exchange.admitted, exchange.call_id);
    if let Some(frames) = &frames
        && frames
            .send(frame(&chunk(&identity, json!({"role": "assistant"}), None)))
            .await
            .is_err()
    {
        outcome.cancelled = true;
        return outcome;
    }

    let heartbeat = exchange.heartbeat.max(Duration::from_millis(1));
    let mut tick = tokio::time::interval(
        (heartbeat / 4).clamp(Duration::from_millis(5), Duration::from_millis(250)),
    );
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut lines = SseLineBuffer::new();
    let mut finish_reason: Option<String> = None;
    let mut done = false;
    let mut last_emit = Instant::now();
    let mut progressed = false;

    'read: loop {
        tokio::select! {
            biased;
            () = &mut closed => {
                outcome.cancelled = true;
                break 'read;
            }
            read = response.chunk() => {
                let bytes = match read {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => break 'read,
                    Err(error) => {
                        outcome.error = Some(ValveError::UpstreamStream(error.to_string()));
                        break 'read;
                    }
                };
                progressed = true;
                let mut deltas = Vec::new();
                for line in lines.push(&bytes) {
                    match classify(&line) {
                        SseLine::Data(value) => {
                            if let Some(error) = value.get("error") {
                                outcome.error = Some(ValveError::UpstreamStream(error.to_string()));
                                break 'read;
                            }
                            outcome.stats.absorb(&value);
                            let choice = value.pointer("/choices/0");
                            if let Some(text) = choice.and_then(|c| c.pointer("/delta/reasoning_content")).and_then(Value::as_str) {
                                deltas.extend(assembler.push_reasoning(text));
                            }
                            if let Some(text) = choice.and_then(|c| c.pointer("/delta/content")).and_then(Value::as_str) {
                                deltas.extend(assembler.push_content(text));
                            }
                            if let Some(reason) = choice.and_then(|c| c.get("finish_reason")).and_then(Value::as_str) {
                                finish_reason = Some(reason.to_string());
                            }
                        }
                        SseLine::Done => done = true,
                        SseLine::Other => {}
                    }
                }
                if let Some(frames) = &frames
                    && !deltas.is_empty()
                {
                    for delta in deltas {
                        if frames.send(frame(&chunk(&identity, delta, None))).await.is_err() {
                            outcome.cancelled = true;
                            break 'read;
                        }
                    }
                    last_emit = Instant::now();
                    progressed = false;
                }
                if done {
                    break 'read;
                }
            }
            _ = tick.tick() => {
                // A heartbeat reports real upstream progress only: a wedged
                // upstream stays silent, so the client's stall detector works.
                if let Some(frames) = &frames
                    && progressed
                    && last_emit.elapsed() >= heartbeat
                {
                    if frames.send(frame(&chunk(&identity, json!({}), None))).await.is_err() {
                        outcome.cancelled = true;
                        break 'read;
                    }
                    last_emit = Instant::now();
                    progressed = false;
                }
            }
        }
    }
    // Dropping the response here closes the upstream connection.
    drop(response);

    if outcome.cancelled {
        return outcome;
    }
    if outcome.error.is_none() && !done && finish_reason.is_none() {
        outcome.error = Some(ValveError::UpstreamStream(
            "upstream stream ended before a finish reason".to_string(),
        ));
    }
    if outcome.error.is_none() {
        match assembler.finish(finish_reason.as_deref()) {
            Ok(resolution) => {
                if let Some(frames) = &frames {
                    let mut closing: Vec<String> = resolution
                        .closing_deltas
                        .iter()
                        .map(|delta| frame(&chunk(&identity, delta.clone(), None)))
                        .collect();
                    closing.push(frame(&chunk(
                        &identity,
                        json!({}),
                        Some(&resolution.finish_reason),
                    )));
                    closing.push(done_frame());
                    for text in closing {
                        if frames.send(text).await.is_err() {
                            outcome.cancelled = true;
                            return outcome;
                        }
                    }
                }
                outcome.resolution = Some(resolution);
            }
            Err(error) => outcome.error = Some(error.into()),
        }
    }
    if let (Some(error), Some(frames)) = (&outcome.error, &frames)
        && frames.send(frame(&error.body())).await.is_err()
    {
        outcome.cancelled = true;
    }
    outcome
}

fn fail_before_start(
    mut outcome: ExchangeOutcome,
    started: Option<oneshot::Sender<Result<(), ValveError>>>,
    error: ValveError,
) -> ExchangeOutcome {
    match started {
        // The server renders the 502 from this error; the outcome keeps a copy
        // of its class for the receipt.
        Some(started) => {
            let _ = started.send(Err(error.clone()));
            outcome.error = Some(error);
        }
        None => outcome.error = Some(error),
    }
    outcome
}

/// The non-streaming `chat.completion` body for a resolved exchange.
pub fn completion_object(
    identity: &ChunkIdentity,
    resolution: &Resolution,
    stats: &UpstreamStats,
) -> Value {
    let mut body = json!({
        "id": identity.id,
        "object": "chat.completion",
        "created": identity.created,
        "model": identity.model,
        "choices": [{
            "index": 0,
            "message": resolution.message,
            "finish_reason": resolution.finish_reason,
        }],
    });
    if let (Some(prompt), Some(completion)) = (stats.prompt_tokens, stats.predicted_tokens) {
        body["usage"] = json!({
            "prompt_tokens": prompt,
            "completion_tokens": completion,
            "total_tokens": prompt + completion,
        });
    }
    body
}
