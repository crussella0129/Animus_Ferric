//! A scripted fake upstream for the valve's model-free integration tests.
//!
//! It speaks just enough of llama.cpp's `/v1/chat/completions` to exercise the
//! valve: scripted SSE with per-frame delays, a non-2xx status, or a JSON body.
//! It records every request body and notices when the valve abandons a stream
//! (the body stream is dropped before the script finished). That is the
//! observable proof that a client disconnect reached the upstream.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone, Debug)]
pub enum Reply {
    /// SSE frames, each sent after its delay.
    Sse(Vec<(Duration, String)>),
    Status(u16, String),
    Json(Value),
}

#[derive(Clone)]
struct Shared {
    replies: Arc<Mutex<Vec<Reply>>>,
    requests: Arc<Mutex<Vec<Value>>>,
    abandoned: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}

pub struct FakeUpstream {
    pub addr: SocketAddr,
    requests: Arc<Mutex<Vec<Value>>>,
    abandoned: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}

impl FakeUpstream {
    /// Serve `replies` in order, one per request; the last one repeats.
    pub async fn start(replies: Vec<Reply>) -> Self {
        let shared = Shared {
            replies: Arc::new(Mutex::new(replies)),
            requests: Arc::default(),
            abandoned: Arc::default(),
            completed: Arc::default(),
        };
        let app = Router::new()
            .route("/v1/chat/completions", post(handle))
            .with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            requests: shared.requests,
            abandoned: shared.abandoned,
            completed: shared.completed,
        }
    }

    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn url(&self) -> String {
        format!("http://{}/v1/chat/completions", self.addr)
    }

    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    /// True once a streaming reply was dropped before its script finished.
    pub fn abandoned(&self) -> bool {
        self.abandoned.load(Ordering::SeqCst)
    }

    pub fn completed(&self) -> bool {
        self.completed.load(Ordering::SeqCst)
    }
}

async fn handle(State(shared): State<Shared>, Json(body): Json<Value>) -> Response {
    shared.requests.lock().unwrap().push(body);
    let reply = {
        let mut replies = shared.replies.lock().unwrap();
        if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies[0].clone()
        }
    };
    match reply {
        Reply::Status(status, text) => Response::builder()
            .status(StatusCode::from_u16(status).unwrap())
            .body(Body::from(text))
            .unwrap(),
        Reply::Json(value) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        Reply::Sse(frames) => {
            let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(4);
            let abandoned = shared.abandoned.clone();
            let completed = shared.completed.clone();
            tokio::spawn(async move {
                for (delay, text) in frames {
                    tokio::select! {
                        () = tx.closed() => {
                            abandoned.store(true, Ordering::SeqCst);
                            return;
                        }
                        () = tokio::time::sleep(delay) => {}
                    }
                    if tx.send(Ok(Bytes::from(text))).await.is_err() {
                        abandoned.store(true, Ordering::SeqCst);
                        return;
                    }
                }
                completed.store(true, Ordering::SeqCst);
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        }
    }
}

pub fn content_frame(text: &str) -> String {
    let value = json!({
        "model": "fake-model",
        "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
    });
    format!("data: {value}\n\n")
}

/// The closing frame, carrying llama.cpp-style `timings`.
pub fn finish_frame(reason: &str) -> String {
    let value = json!({
        "model": "fake-model",
        "choices": [{"index": 0, "delta": {}, "finish_reason": reason}],
        "timings": {
            "prompt_n": 10, "prompt_ms": 50.0,
            "predicted_n": 20, "predicted_ms": 400.0,
            "cache_n": 90
        }
    });
    format!("data: {value}\n\n")
}

pub fn done_frame() -> String {
    "data: [DONE]\n\n".to_string()
}

/// `text` split into `piece`-char content frames `delay` apart, then finish and
/// `[DONE]`.
pub fn stream_of(text: &str, piece: usize, delay: Duration, finish: &str) -> Reply {
    let chars: Vec<char> = text.chars().collect();
    let mut frames: Vec<(Duration, String)> = chars
        .chunks(piece)
        .map(|chunk| (delay, content_frame(&chunk.iter().collect::<String>())))
        .collect();
    frames.push((Duration::ZERO, finish_frame(finish)));
    frames.push((Duration::ZERO, done_frame()));
    Reply::Sse(frames)
}

pub const TOOL_ACTION: &str =
    r#"{"thought":"read the notes","tool":"read_file","args":{"path":"notes.txt"}}"#;
pub const FINAL_ACTION: &str =
    r#"{"thought":"answer now","tool":"task_complete","args":{"summary":"It says: buy milk."}}"#;
