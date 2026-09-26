//! T-12606: the constrained exchange against a scripted fake upstream
//! (INT-0012 AC-1/2/3). Model-free, loopback only, bounded timeouts.

mod common;

use std::time::Duration;

use common::{
    FINAL_ACTION, FakeUpstream, Reply, TOOL_ACTION, content_frame, done_frame, finish_frame,
    stream_of,
};
use ferric_valve::sse::ChunkIdentity;
use ferric_valve::{Exchange, ExchangeOutcome, ValveError, run_constrained};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

const LIMIT: Duration = Duration::from_secs(10);

fn identity() -> ChunkIdentity {
    ChunkIdentity {
        id: "chatcmpl-test".to_string(),
        created: 0,
        model: "fake".to_string(),
    }
}

fn body() -> Value {
    json!({"model": "fake", "stream": true, "messages": [{"role": "user", "content": "hi"}]})
}

fn exchange<'a>(
    client: &'a reqwest::Client,
    url: String,
    body: &'a Value,
    heartbeat: Duration,
) -> Exchange<'a> {
    Exchange {
        client,
        url,
        authorization: None,
        body,
        admitted: vec!["read_file".to_string(), "task_complete".to_string()],
        call_id: "call_test".to_string(),
        identity: identity(),
        heartbeat,
    }
}

/// Run a streaming exchange to completion; return the parsed data frames (the
/// literal `[DONE]` becomes `Value::String("[DONE]")`) and the outcome.
async fn run_streaming(
    url: String,
    heartbeat: Duration,
) -> (Vec<Value>, ExchangeOutcome, Result<(), ValveError>) {
    let client = reqwest::Client::new();
    let body = body();
    let (tx, mut rx) = mpsc::channel::<String>(256);
    let (started_tx, started_rx) = oneshot::channel();
    let closed = {
        let tx = tx.clone();
        async move { tx.closed().await }
    };
    let collector = tokio::spawn(async move {
        let mut frames = Vec::new();
        while let Some(text) = rx.recv().await {
            let payload = text.strip_prefix("data: ").unwrap().trim_end().to_string();
            frames.push(if payload == "[DONE]" {
                Value::String(payload)
            } else {
                serde_json::from_str(&payload).unwrap()
            });
        }
        frames
    });
    let outcome = tokio::time::timeout(
        LIMIT,
        run_constrained(
            exchange(&client, url, &body, heartbeat),
            Some(tx),
            Some(started_tx),
            closed,
        ),
    )
    .await
    .expect("exchange finished");
    let frames = tokio::time::timeout(LIMIT, collector)
        .await
        .unwrap()
        .unwrap();
    let started = started_rx.await.expect("start signalled");
    (frames, outcome, started)
}

fn deltas(frames: &[Value]) -> Vec<&Value> {
    frames
        .iter()
        .filter_map(|frame| frame.pointer("/choices/0/delta"))
        .collect()
}

fn is_heartbeat(frame: &Value) -> bool {
    frame.pointer("/choices/0/delta") == Some(&json!({}))
        && frame.pointer("/choices/0/finish_reason") == Some(&Value::Null)
}

/// Assemble streamed deltas the way an OpenAI client does.
fn assemble(frames: &[Value]) -> Value {
    let mut reasoning = String::new();
    let mut content: Option<String> = None;
    let mut call: Option<Value> = None;
    for delta in deltas(frames) {
        if let Some(text) = delta["reasoning_content"].as_str() {
            reasoning.push_str(text);
        }
        if let Some(text) = delta["content"].as_str() {
            content.get_or_insert_default().push_str(text);
        }
        if let Some(part) = delta["tool_calls"].get(0) {
            let entry = call.get_or_insert_with(|| {
                json!({"id": part["id"], "type": "function", "function": {"name": "", "arguments": ""}})
            });
            if let Some(name) = part.pointer("/function/name").and_then(Value::as_str) {
                entry["function"]["name"] = json!(name);
            }
            if let Some(arguments) = part.pointer("/function/arguments").and_then(Value::as_str) {
                let joined = format!(
                    "{}{arguments}",
                    entry["function"]["arguments"].as_str().unwrap()
                );
                entry["function"]["arguments"] = json!(joined);
            }
        }
    }
    let mut message = json!({"role": "assistant", "content": content});
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if let Some(call) = call {
        message["tool_calls"] = json!([call]);
    }
    message
}

#[tokio::test(flavor = "multi_thread")]
async fn stream_tool_call_end_to_end() {
    let upstream =
        FakeUpstream::start(vec![stream_of(TOOL_ACTION, 4, Duration::ZERO, "stop")]).await;
    let (frames, outcome, started) = run_streaming(upstream.url(), Duration::from_secs(5)).await;
    assert!(started.is_ok());
    let resolution = outcome.resolution.expect("resolved");
    assert_eq!(resolution.finish_reason, "tool_calls");
    assert_eq!(resolution.tool.as_deref(), Some("read_file"));
    assert_eq!(
        assemble(&frames)["tool_calls"][0]["function"],
        json!({"name": "read_file", "arguments": "{\"path\":\"notes.txt\"}"})
    );
    assert_eq!(outcome.stats.prompt_eval_tokens, Some(10));
    assert_eq!(outcome.stats.cached_tokens, Some(90));
    assert_eq!(outcome.stats.predicted_tokens, Some(20));
    assert_eq!(outcome.stats.prompt_tokens, Some(100));
    // The upstream really received a streaming request.
    assert_eq!(upstream.requests()[0]["stream"], json!(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn stream_final_answer_end_to_end() {
    let upstream =
        FakeUpstream::start(vec![stream_of(FINAL_ACTION, 4, Duration::ZERO, "stop")]).await;
    let (frames, outcome, _) = run_streaming(upstream.url(), Duration::from_secs(5)).await;
    let resolution = outcome.resolution.expect("resolved");
    assert_eq!(resolution.finish_reason, "stop");
    let message = assemble(&frames);
    assert_eq!(message["content"], "It says: buy milk.");
    assert_eq!(message["reasoning_content"], "answer now");
    assert!(message.get("tool_calls").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn stream_chunk_order() {
    let upstream =
        FakeUpstream::start(vec![stream_of(TOOL_ACTION, 3, Duration::ZERO, "stop")]).await;
    let (frames, _, _) = run_streaming(upstream.url(), Duration::from_secs(5)).await;
    // role → reasoning… → tool name → arguments → finish → [DONE]
    assert_eq!(
        frames[0].pointer("/choices/0/delta"),
        Some(&json!({"role": "assistant"}))
    );
    let kinds: Vec<&str> = frames
        .iter()
        .map(|frame| {
            if frame == &Value::String("[DONE]".to_string()) {
                return "done";
            }
            let delta = &frame["choices"][0]["delta"];
            if frame["choices"][0]["finish_reason"].is_string() {
                "finish"
            } else if delta.get("role").is_some() {
                "role"
            } else if delta.get("reasoning_content").is_some() {
                "reasoning"
            } else if delta.pointer("/tool_calls/0/function/name").is_some() {
                "name"
            } else if delta.pointer("/tool_calls/0/function/arguments").is_some() {
                "args"
            } else {
                "other"
            }
        })
        .collect();
    let first = |kind: &str| kinds.iter().position(|k| *k == kind).unwrap();
    let last = |kind: &str| kinds.iter().rposition(|k| *k == kind).unwrap();
    assert_eq!(first("role"), 0);
    assert!(last("reasoning") < first("name"));
    assert!(first("name") < first("args"));
    assert_eq!(kinds.iter().filter(|k| **k == "name").count(), 1);
    assert_eq!(kinds[kinds.len() - 2], "finish");
    assert_eq!(kinds[kinds.len() - 1], "done");
    assert_eq!(
        frames[kinds.len() - 2]["choices"][0]["finish_reason"],
        "tool_calls"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn nonstream_equals_assembled_stream() {
    for action in [TOOL_ACTION, FINAL_ACTION] {
        let upstream =
            FakeUpstream::start(vec![stream_of(action, 5, Duration::ZERO, "stop")]).await;
        let (frames, streamed, _) = run_streaming(upstream.url(), Duration::from_secs(5)).await;

        let client = reqwest::Client::new();
        let body = body();
        let (alive_tx, _alive_rx) = mpsc::channel::<()>(1);
        let closed = async move { alive_tx.closed().await };
        let plain = tokio::time::timeout(
            LIMIT,
            run_constrained(
                exchange(&client, upstream.url(), &body, Duration::from_secs(5)),
                None,
                None,
                closed,
            ),
        )
        .await
        .unwrap();
        let plain_message = plain.resolution.expect("resolved").message;
        assert_eq!(plain_message, streamed.resolution.unwrap().message);
        assert_eq!(assemble(&frames), plain_message, "for {action}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_only_on_upstream_progress() {
    // After the tool name, argument bytes keep arriving but produce no live
    // delta: upstream progress without downstream output, so heartbeats flow.
    let mut frames = vec![(
        Duration::ZERO,
        content_frame(r#"{"thought":"t","tool":"read_file","args":{"path":""#),
    )];
    for _ in 0..12 {
        frames.push((Duration::from_millis(30), content_frame("a")));
    }
    frames.push((Duration::ZERO, content_frame(r#""}}"#)));
    frames.push((Duration::ZERO, finish_frame("stop")));
    frames.push((Duration::ZERO, done_frame()));
    let upstream = FakeUpstream::start(vec![Reply::Sse(frames)]).await;
    let (frames, outcome, _) = run_streaming(upstream.url(), Duration::from_millis(60)).await;
    assert!(outcome.resolution.is_some());
    let heartbeats = frames.iter().filter(|frame| is_heartbeat(frame)).count();
    assert!(
        heartbeats >= 1,
        "expected heartbeats while arguments stream, got {heartbeats}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn no_heartbeat_when_upstream_stalls() {
    // Everything after the stall arrives in one write, so any heartbeat could
    // only have been emitted during the 500 ms in which no byte arrived.
    let after_stall = format!(
        "{}{}{}",
        content_frame(r#""path":"a"}}"#),
        finish_frame("stop"),
        done_frame()
    );
    let frames = vec![
        (
            Duration::ZERO,
            content_frame(r#"{"thought":"t","tool":"read_file","args":{"#),
        ),
        (Duration::from_millis(500), after_stall),
    ];
    let upstream = FakeUpstream::start(vec![Reply::Sse(frames)]).await;
    let (frames, outcome, _) = run_streaming(upstream.url(), Duration::from_millis(50)).await;
    assert!(outcome.resolution.is_some());
    assert_eq!(
        frames.iter().filter(|frame| is_heartbeat(frame)).count(),
        0,
        "a silent upstream must not be dressed up as progress"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn length_truncation_is_reported() {
    let truncated = r#"{"thought":"t","tool":"task_complete","args":{"summary":"partial ans"#;
    let upstream =
        FakeUpstream::start(vec![stream_of(truncated, 6, Duration::ZERO, "length")]).await;
    let (frames, outcome, _) = run_streaming(upstream.url(), Duration::from_secs(5)).await;
    let resolution = outcome.resolution.expect("resolved as truncated");
    assert_eq!(resolution.finish_reason, "length");
    assert!(!resolution.action_valid);
    let message = assemble(&frames);
    assert_eq!(message["content"], Value::Null, "no partial answer leaks");
    assert!(message.get("tool_calls").is_none());
    assert_eq!(
        frames[frames.len() - 2]["choices"][0]["finish_reason"],
        "length"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_upstream_reports_error_in_band() {
    let upstream = FakeUpstream::start(vec![stream_of(
        "Sure! Here is the file.",
        6,
        Duration::ZERO,
        "stop",
    )])
    .await;
    let (frames, outcome, started) = run_streaming(upstream.url(), Duration::from_secs(5)).await;
    assert!(started.is_ok(), "the upstream itself answered 200");
    assert!(matches!(outcome.error, Some(ValveError::Action(_))));
    let last = frames.last().unwrap();
    assert_eq!(last["error"]["code"], "unparsable_action");
    assert!(!frames.contains(&Value::String("[DONE]".to_string())));
    assert!(
        assemble(&frames)["content"].is_null(),
        "raw text is never returned as an answer"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn upstream_http_error_is_reported_before_start() {
    let upstream = FakeUpstream::start(vec![Reply::Status(
        400,
        "unsupported schema construct".to_string(),
    )])
    .await;
    let (frames, outcome, started) = run_streaming(upstream.url(), Duration::from_secs(5)).await;
    assert!(
        frames.is_empty(),
        "nothing streams before the upstream accepts"
    );
    match started {
        Err(ValveError::UpstreamHttp { status, excerpt }) => {
            assert_eq!(status, 400);
            assert!(excerpt.contains("unsupported schema construct"));
        }
        other => panic!("unexpected start signal: {other:?}"),
    }
    assert_eq!(outcome.error.unwrap().class(), "upstream_http");
    assert_eq!(upstream.requests().len(), 1, "no retry");
}

#[tokio::test(flavor = "multi_thread")]
async fn upstream_unreachable_is_reported_before_start() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let (_, outcome, started) = run_streaming(
        format!("http://{addr}/v1/chat/completions"),
        Duration::from_secs(5),
    )
    .await;
    assert!(matches!(started, Err(ValveError::UpstreamUnreachable(_))));
    assert_eq!(outcome.error.unwrap().class(), "upstream_unreachable");
}

#[tokio::test(flavor = "multi_thread")]
async fn client_disconnect_closes_upstream() {
    // A long, slow stream: 200 frames 50 ms apart would take 10 s to finish.
    let mut frames = vec![(Duration::ZERO, content_frame(r#"{"thought":""#))];
    for _ in 0..200 {
        frames.push((Duration::from_millis(50), content_frame("x")));
    }
    let upstream = FakeUpstream::start(vec![Reply::Sse(frames)]).await;
    let client = reqwest::Client::new();
    let body = body();
    let url = upstream.url();
    let (tx, mut rx) = mpsc::channel::<String>(256);
    let closed = {
        let tx = tx.clone();
        async move { tx.closed().await }
    };
    let task = tokio::spawn(async move {
        run_constrained(
            exchange(&client, url, &body, Duration::from_secs(5)),
            Some(tx),
            None,
            closed,
        )
        .await
    });
    // Read the role chunk and a few thought deltas, then hang up.
    for _ in 0..3 {
        tokio::time::timeout(LIMIT, rx.recv())
            .await
            .unwrap()
            .unwrap();
    }
    drop(rx);
    let outcome = tokio::time::timeout(LIMIT, task).await.unwrap().unwrap();
    assert!(outcome.cancelled);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !upstream.abandoned() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "upstream never saw the connection close"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!upstream.completed());
}
