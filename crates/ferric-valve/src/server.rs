//! The valve's HTTP surface (T-12607, INT-0012).
//!
//! `POST /v1/chat/completions` (and `/chat/completions`) goes through the
//! constrained transform, or is forwarded byte-for-byte in pass-through and
//! record-only modes. Every other path is forwarded unchanged; Hermes may
//! query `/v1/models` or `/props`. Each chat request runs in its own task that
//! owns the upstream exchange and writes exactly one receipt, so a client
//! hanging up still produces a `cancelled` receipt rather than vanishing with a
//! dropped handler.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::post;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::ReceiverStream;

use crate::receipt::{Receipt, ReceiptSink};
use crate::sse::{ChunkIdentity, SseLine, SseLineBuffer, classify};
use crate::transform::{
    Constrained, Transformed, message_hashes, prefix_hash, sha256_hex, transform,
};
use crate::upstream::{Exchange, UpstreamStats, ValveError, completion_object, run_constrained};

/// Whether the valve constrains eligible requests or only records them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValveMode {
    Constrained,
    /// Forward every request byte-for-byte, writing the same receipts: the
    /// native comparison arm through the same code path.
    RecordOnly,
}

#[derive(Debug, Clone)]
pub struct ValveConfig {
    /// Upstream origin, e.g. `http://127.0.0.1:8080` (no trailing slash).
    pub upstream: String,
    pub mode: ValveMode,
    /// Minimum spacing of progress heartbeats on a constrained stream.
    pub heartbeat: Duration,
}

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    config: Arc<ValveConfig>,
    receipts: Arc<ReceiptSink>,
    counter: Arc<AtomicU64>,
}

/// The default listen address: loopback only.
pub fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8090))
}

/// The valve has no network-exposure features: it binds loopback only.
pub fn check_loopback(addr: &SocketAddr) -> Result<(), String> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(format!(
            "refusing to listen on {addr}: the valve binds loopback addresses only"
        ))
    }
}

/// An HTTP client for the (loopback) upstream: no proxies, no redirects.
pub fn upstream_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

pub fn router(config: ValveConfig, receipts: ReceiptSink) -> Result<Router, reqwest::Error> {
    let state = AppState {
        client: upstream_client()?,
        config: Arc::new(ValveConfig {
            upstream: config.upstream.trim_end_matches('/').to_string(),
            ..config
        }),
        receipts: Arc::new(receipts),
        counter: Arc::default(),
    };
    Ok(Router::new()
        .route("/v1/chat/completions", post(chat))
        .route("/chat/completions", post(chat))
        .fallback(proxy)
        .with_state(state))
}

/// Serve `router` on `listener` until the process ends.
pub async fn serve(listener: tokio::net::TcpListener, router: Router) -> std::io::Result<()> {
    axum::serve(listener, router).await
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn json_response(status: StatusCode, value: &Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap_or_default()
}

fn sse_response(rx: mpsc::Receiver<String>) -> Response {
    let body = Body::from_stream(
        ReceiverStream::new(rx).map(|text| Ok::<Bytes, Infallible>(Bytes::from(text))),
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .unwrap_or_default()
}

/// Content-free identities of a request as received (for forwarded modes).
fn identify_request(receipt: &mut Receipt, request: &Value) {
    if let Some(messages) = request.get("messages").and_then(Value::as_array) {
        receipt.message_hashes = message_hashes(messages);
        receipt.prefix_hash = Some(prefix_hash(messages));
    }
    if let Some(tools) = request.get("tools").filter(|tools| tools.is_array()) {
        receipt.tools_offered = tools.as_array().map(Vec::len);
        receipt.tool_catalog_hash =
            Some(sha256_hex(&serde_json::to_vec(tools).unwrap_or_default()));
    }
}

/// The first tool name in a response object or chunk, if any.
fn first_tool_name(value: &Value) -> Option<String> {
    value
        .pointer("/choices/0/message/tool_calls/0/function/name")
        .or_else(|| value.pointer("/choices/0/delta/tool_calls/0/function/name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let started_unix_ms = now_unix_ms();
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "valve-{started_unix_ms}-{}",
                state.counter.fetch_add(1, Ordering::Relaxed)
            )
        });
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let path = uri
        .path_and_query()
        .map_or("/v1/chat/completions", |path| path.as_str());
    let url = format!("{}{}", state.config.upstream, path);
    let parsed: Option<Value> = serde_json::from_slice(&body).ok();
    let receipt = Receipt {
        request_id: request_id.clone(),
        stream: parsed
            .as_ref()
            .and_then(|request| request.get("stream"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        started_unix_ms,
        ..Receipt::default()
    };
    let job = Job {
        state,
        url,
        authorization,
        started,
        receipt,
    };

    let Some(request) = parsed else {
        return forward(job, body, None, "pass-through").await;
    };
    if job.state.config.mode == ValveMode::RecordOnly {
        return forward(job, body, Some(request), "record-only").await;
    }
    match transform(&request) {
        Ok(Transformed::PassThrough) => forward(job, body, Some(request), "pass-through").await,
        Ok(Transformed::Constrained(constrained)) => {
            let model = request
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("ferric-valve")
                .to_string();
            constrain(job, *constrained, model, request_id).await
        }
        Err(error) => {
            let mut receipt = job.receipt;
            receipt.mode = "constrained".to_string();
            receipt.outcome = "failed".to_string();
            receipt.error_class = Some("transform".to_string());
            receipt.http_status = Some(400);
            receipt.wall_ms = job.started.elapsed().as_millis() as u64;
            let _ = job.state.receipts.write(&receipt);
            json_response(
                StatusCode::BAD_REQUEST,
                &json!({"error": {
                    "message": error.to_string(),
                    "type": "invalid_request_error",
                    "code": "ferric_valve_transform"
                }}),
            )
        }
    }
}

struct Job {
    state: AppState,
    url: String,
    authorization: Option<String>,
    started: Instant,
    receipt: Receipt,
}

impl Job {
    fn finish(mut self, outcome: &str) {
        self.receipt.outcome = outcome.to_string();
        self.receipt.wall_ms = self.started.elapsed().as_millis() as u64;
        if let Err(error) = self.state.receipts.write(&self.receipt) {
            eprintln!("ferric-valve: failed to write receipt: {error}");
        }
    }
}

async fn constrain(
    mut job: Job,
    constrained: Constrained,
    model: String,
    request_id: String,
) -> Response {
    let messages = constrained.upstream_messages();
    job.receipt.mode = "constrained".to_string();
    job.receipt.tools_offered = Some(constrained.tools_offered);
    job.receipt.tool_catalog_hash = Some(constrained.tool_catalog_hash.clone());
    job.receipt.schema_hash = Some(constrained.schema_hash.clone());
    job.receipt.message_hashes = message_hashes(messages);
    let prefix = prefix_hash(messages);
    job.receipt.prefix_hash = Some(prefix.clone());

    let identity = ChunkIdentity {
        id: format!("chatcmpl-{request_id}"),
        created: now_unix_ms() / 1_000,
        model,
    };
    let call_id = format!("call_{}", &prefix[..24]);
    let client_stream = constrained.client_stream;

    if client_stream {
        let (tx, rx) = mpsc::channel::<String>(64);
        let (started_tx, started_rx) = oneshot::channel();
        tokio::spawn(async move {
            let closed = {
                let tx = tx.clone();
                async move { tx.closed().await }
            };
            let exchange = Exchange {
                client: &job.state.client,
                url: job.url.clone(),
                authorization: job.authorization.clone(),
                body: &constrained.upstream_body,
                admitted: constrained.admitted.clone(),
                call_id,
                identity,
                heartbeat: job.state.config.heartbeat,
            };
            let outcome = run_constrained(exchange, Some(tx), Some(started_tx), closed).await;
            record_constrained(job, outcome);
        });
        return match started_rx.await {
            Ok(Ok(())) => sse_response(rx),
            Ok(Err(error)) => json_response(StatusCode::BAD_GATEWAY, &error.body()),
            Err(_) => json_response(
                StatusCode::BAD_GATEWAY,
                &ValveError::UpstreamStream("exchange ended before it started".to_string()).body(),
            ),
        };
    }

    let (alive_tx, alive_rx) = mpsc::channel::<()>(1);
    let (done_tx, done_rx) = oneshot::channel();
    let body_identity = identity.clone();
    tokio::spawn(async move {
        let closed = async move { alive_tx.closed().await };
        let exchange = Exchange {
            client: &job.state.client,
            url: job.url.clone(),
            authorization: job.authorization.clone(),
            body: &constrained.upstream_body,
            admitted: constrained.admitted.clone(),
            call_id,
            identity,
            heartbeat: job.state.config.heartbeat,
        };
        let outcome = run_constrained(exchange, None, None, closed).await;
        let reply = match (&outcome.resolution, &outcome.error) {
            (Some(resolution), _) => Ok(completion_object(
                &body_identity,
                resolution,
                &outcome.stats,
            )),
            (None, Some(error)) => Err(error.body()),
            (None, None) => Err(ValveError::UpstreamStream("cancelled".to_string()).body()),
        };
        record_constrained(job, outcome);
        let _ = done_tx.send(reply);
    });
    let _alive = alive_rx;
    match done_rx.await {
        Ok(Ok(body)) => json_response(StatusCode::OK, &body),
        Ok(Err(error)) => json_response(StatusCode::BAD_GATEWAY, &error),
        Err(_) => json_response(
            StatusCode::BAD_GATEWAY,
            &ValveError::UpstreamStream("exchange ended without a result".to_string()).body(),
        ),
    }
}

fn record_constrained(mut job: Job, outcome: crate::upstream::ExchangeOutcome) {
    job.receipt.absorb_stats(&outcome.stats);
    let verdict = if let Some(resolution) = &outcome.resolution {
        job.receipt.finish_reason = Some(resolution.finish_reason.clone());
        job.receipt.action_valid = Some(resolution.action_valid);
        job.receipt.tool = resolution.tool.clone();
        job.receipt.http_status = Some(200);
        "completed"
    } else if outcome.cancelled {
        "cancelled"
    } else {
        let error = outcome.error.as_ref();
        job.receipt.error_class = error.map(|error| error.class().to_string());
        let before_start = matches!(
            error,
            Some(ValveError::UpstreamHttp { .. } | ValveError::UpstreamUnreachable(_))
        );
        job.receipt.http_status = Some(if before_start || !job.receipt.stream {
            502
        } else {
            200
        });
        if matches!(error, Some(ValveError::Action(_))) {
            job.receipt.action_valid = Some(false);
        }
        "failed"
    };
    job.finish(verdict);
}

enum Forwarded {
    Stream {
        status: StatusCode,
        content_type: Option<HeaderValue>,
        rx: mpsc::Receiver<Bytes>,
    },
    Full {
        status: StatusCode,
        content_type: Option<HeaderValue>,
        bytes: Bytes,
    },
    Failed(Value),
}

/// Forward the original bytes, streaming preserved, while reading the reply
/// passively for the receipt.
async fn forward(mut job: Job, body: Bytes, request: Option<Value>, mode: &str) -> Response {
    job.receipt.mode = mode.to_string();
    if let Some(request) = &request {
        identify_request(&mut job.receipt, request);
    }
    let client_stream = job.receipt.stream;
    let (alive_tx, alive_rx) = mpsc::channel::<()>(1);
    let (reply_tx, reply_rx) = oneshot::channel::<Forwarded>();

    tokio::spawn(async move {
        let mut outbound = job
            .state
            .client
            .post(&job.url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(authorization) = &job.authorization {
            outbound = outbound.header(header::AUTHORIZATION, authorization);
        }
        let sent = tokio::select! {
            biased;
            () = alive_tx.closed() => return job.finish("cancelled"),
            sent = outbound.send() => sent,
        };
        let mut response = match sent {
            Ok(response) => response,
            Err(error) => {
                let error = ValveError::UpstreamUnreachable(error.to_string());
                job.receipt.error_class = Some(error.class().to_string());
                job.receipt.http_status = Some(502);
                let _ = reply_tx.send(Forwarded::Failed(error.body()));
                return job.finish("failed");
            }
        };
        let status = response.status();
        let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
        job.receipt.http_status = Some(status.as_u16());
        if !status.is_success() {
            job.receipt.error_class = Some("upstream_http".to_string());
        }
        let mut stats = UpstreamStats::default();

        if client_stream && status.is_success() {
            let (tx, rx) = mpsc::channel::<Bytes>(64);
            if reply_tx
                .send(Forwarded::Stream {
                    status,
                    content_type,
                    rx,
                })
                .is_err()
            {
                return job.finish("cancelled");
            }
            let mut lines = SseLineBuffer::new();
            let mut verdict = "completed";
            loop {
                tokio::select! {
                    biased;
                    () = tx.closed() => {
                        verdict = "cancelled";
                        break;
                    }
                    read = response.chunk() => match read {
                        Ok(Some(bytes)) => {
                            for line in lines.push(&bytes) {
                                if let SseLine::Data(value) = classify(&line) {
                                    stats.absorb(&value);
                                    if let Some(reason) = value.pointer("/choices/0/finish_reason").and_then(Value::as_str) {
                                        job.receipt.finish_reason = Some(reason.to_string());
                                    }
                                    if job.receipt.tool.is_none() {
                                        job.receipt.tool = first_tool_name(&value);
                                    }
                                }
                            }
                            if tx.send(bytes).await.is_err() {
                                verdict = "cancelled";
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            job.receipt.error_class = Some("upstream_stream".to_string());
                            verdict = "failed";
                            break;
                        }
                    }
                }
            }
            drop(response);
            job.receipt.absorb_stats(&stats);
            return job.finish(verdict);
        }

        let bytes = tokio::select! {
            biased;
            () = alive_tx.closed() => return job.finish("cancelled"),
            bytes = response.bytes() => bytes,
        };
        match bytes {
            Ok(bytes) => {
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                    stats.absorb(&value);
                    job.receipt.finish_reason = value
                        .pointer("/choices/0/finish_reason")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    job.receipt.tool = first_tool_name(&value);
                }
                job.receipt.absorb_stats(&stats);
                let verdict = if status.is_success() {
                    "completed"
                } else {
                    "failed"
                };
                let _ = reply_tx.send(Forwarded::Full {
                    status,
                    content_type,
                    bytes,
                });
                job.finish(verdict);
            }
            Err(error) => {
                let error = ValveError::UpstreamStream(error.to_string());
                job.receipt.error_class = Some(error.class().to_string());
                job.receipt.http_status = Some(502);
                let _ = reply_tx.send(Forwarded::Failed(error.body()));
                job.finish("failed");
            }
        }
    });

    let _alive = alive_rx;
    match reply_rx.await {
        Ok(Forwarded::Stream {
            status,
            content_type,
            rx,
        }) => {
            let body = Body::from_stream(ReceiverStream::new(rx).map(Ok::<Bytes, Infallible>));
            let mut response = Response::builder().status(status);
            if let Some(content_type) = content_type {
                response = response.header(header::CONTENT_TYPE, content_type);
            }
            response.body(body).unwrap_or_default()
        }
        Ok(Forwarded::Full {
            status,
            content_type,
            bytes,
        }) => {
            let mut response = Response::builder().status(status);
            if let Some(content_type) = content_type {
                response = response.header(header::CONTENT_TYPE, content_type);
            }
            response.body(Body::from(bytes)).unwrap_or_default()
        }
        Ok(Forwarded::Failed(error)) => json_response(StatusCode::BAD_GATEWAY, &error),
        Err(_) => json_response(
            StatusCode::BAD_GATEWAY,
            &ValveError::UpstreamStream("forwarding ended without a reply".to_string()).body(),
        ),
    }
}

/// Every other path is forwarded unchanged (no receipt).
async fn proxy(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let path = uri.path_and_query().map_or("/", |path| path.as_str());
    let url = format!("{}{}", state.config.upstream, path);
    let Ok(method) = reqwest::Method::from_bytes(method.as_str().as_bytes()) else {
        return json_response(
            StatusCode::METHOD_NOT_ALLOWED,
            &json!({"error": {"message": "unsupported method"}}),
        );
    };
    let mut outbound = state.client.request(method, url).body(body);
    for name in [header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT] {
        if let Some(value) = headers.get(&name) {
            outbound = outbound.header(name, value);
        }
    }
    match outbound.send().await {
        Ok(response) => {
            let status = response.status();
            let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
            let bytes = response.bytes().await.unwrap_or_default();
            let mut reply = Response::builder().status(status);
            if let Some(content_type) = content_type {
                reply = reply.header(header::CONTENT_TYPE, content_type);
            }
            reply.body(Body::from(bytes)).unwrap_or_default()
        }
        Err(error) => json_response(
            StatusCode::BAD_GATEWAY,
            &ValveError::UpstreamUnreachable(error.to_string()).body(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_listen_is_loopback() {
        assert!(default_listen().ip().is_loopback());
        assert!(check_loopback(&default_listen()).is_ok());
    }

    #[test]
    fn non_loopback_listen_is_refused() {
        let public: SocketAddr = "0.0.0.0:8090".parse().unwrap();
        assert!(check_loopback(&public).unwrap_err().contains("loopback"));
        let v6: SocketAddr = "[::1]:8090".parse().unwrap();
        assert!(check_loopback(&v6).is_ok());
    }
}
