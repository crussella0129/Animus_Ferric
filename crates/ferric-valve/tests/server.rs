//! T-12607: the valve's HTTP surface, receipts, probe and record-only mode,
//! against a scripted fake upstream (INT-0012 AC-1/4/5/6). Model-free,
//! loopback only, bounded timeouts, no child processes.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{FINAL_ACTION, FakeUpstream, Reply, TOOL_ACTION, content_frame, stream_of};
use ferric_valve::probe::{ProbeError, probe_enforcement};
use ferric_valve::receipt::ReceiptSink;
use ferric_valve::server::upstream_client;
use ferric_valve::{ValveConfig, ValveMode, router, serve};
use serde_json::{Value, json};

const LIMIT: Duration = Duration::from_secs(10);

struct Valve {
    base: String,
    receipts: PathBuf,
    _dir: tempfile::TempDir,
}

impl Valve {
    fn url(&self) -> String {
        format!("{}/v1/chat/completions", self.base)
    }

    fn receipts(&self) -> Vec<Value> {
        read_receipts(&self.receipts)
    }
}

fn read_receipts(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn start_valve(upstream: &str, mode: ValveMode) -> Valve {
    let dir = tempfile::tempdir().unwrap();
    let receipts = dir.path().join("receipts.jsonl");
    let app = router(
        ValveConfig {
            upstream: upstream.to_string(),
            mode,
            heartbeat: Duration::from_secs(2),
        },
        ReceiptSink::open(&receipts).unwrap(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        serve(listener, app).await.unwrap();
    });
    Valve {
        base,
        receipts,
        _dir: dir,
    }
}

/// Wait (bounded) until `count` receipts have been written.
async fn receipts_eventually(valve: &Valve, count: usize) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + LIMIT;
    loop {
        let receipts = valve.receipts();
        if receipts.len() >= count {
            return receipts;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "only {} receipts",
            receipts.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn hermes_tools() -> Value {
    serde_json::from_str(include_str!("fixtures/hermes_file_tools.json")).unwrap()
}

fn chat_request(stream: bool) -> Value {
    json!({
        "model": "local",
        "stream": stream,
        "messages": [
            {"role": "system", "content": "You are Hermes."},
            {"role": "user", "content": "What is in notes.txt?"}
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a text file.",
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
            }
        }],
        "tool_choice": "auto"
    })
}

async fn post(url: &str, body: &Value) -> reqwest::Response {
    tokio::time::timeout(LIMIT, reqwest::Client::new().post(url).json(body).send())
        .await
        .unwrap()
        .unwrap()
}

async fn text(response: reqwest::Response) -> String {
    tokio::time::timeout(LIMIT, response.text())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn server_maps_transform_error_to_400() {
    let upstream =
        FakeUpstream::start(vec![stream_of(TOOL_ACTION, 8, Duration::ZERO, "stop")]).await;
    let valve = start_valve(&upstream.base(), ValveMode::Constrained).await;
    let mut body = chat_request(false);
    body["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role": "tool", "tool_call_id": "call_missing", "content": "x"}));
    let response = post(&valve.url(), &body).await;
    assert_eq!(response.status(), 400);
    let error: Value = serde_json::from_str(&text(response).await).unwrap();
    assert_eq!(error["error"]["code"], "ferric_valve_transform");
    assert!(upstream.requests().is_empty(), "nothing went upstream");
    let receipts = receipts_eventually(&valve, 1).await;
    assert_eq!(receipts[0]["outcome"], "failed");
    assert_eq!(receipts[0]["error_class"], "transform");
    assert_eq!(receipts[0]["http_status"], 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_upstream_yields_502() {
    let upstream = FakeUpstream::start(vec![stream_of(
        "Sure! It says buy milk.",
        6,
        Duration::ZERO,
        "stop",
    )])
    .await;
    let valve = start_valve(&upstream.base(), ValveMode::Constrained).await;
    let response = post(&valve.url(), &chat_request(false)).await;
    assert_eq!(response.status(), 502);
    let body: Value = serde_json::from_str(&text(response).await).unwrap();
    assert_eq!(body["error"]["code"], "unparsable_action");
    assert!(
        body.get("choices").is_none(),
        "raw text is never returned as an answer"
    );
    let receipts = receipts_eventually(&valve, 1).await;
    assert_eq!(receipts[0]["outcome"], "failed");
    assert_eq!(receipts[0]["action_valid"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn upstream_http_error_yields_502_with_status() {
    for stream in [false, true] {
        let upstream =
            FakeUpstream::start(vec![Reply::Status(400, "unsupported schema".to_string())]).await;
        let valve = start_valve(&upstream.base(), ValveMode::Constrained).await;
        let response = post(&valve.url(), &chat_request(stream)).await;
        assert_eq!(response.status(), 502, "stream={stream}");
        let body: Value = serde_json::from_str(&text(response).await).unwrap();
        assert_eq!(body["error"]["code"], "upstream_http");
        assert_eq!(body["error"]["upstream_status"], 400);
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unsupported schema")
        );
        assert_eq!(upstream.requests().len(), 1, "no retry");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn upstream_unreachable_yields_502() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let valve = start_valve(&format!("http://{addr}"), ValveMode::Constrained).await;
    let response = post(&valve.url(), &chat_request(false)).await;
    assert_eq!(response.status(), 502);
    let body: Value = serde_json::from_str(&text(response).await).unwrap();
    assert_eq!(body["error"]["code"], "upstream_unreachable");
}

#[tokio::test(flavor = "multi_thread")]
async fn one_receipt_per_request() {
    let upstream = FakeUpstream::start(vec![
        stream_of(FINAL_ACTION, 8, Duration::ZERO, "stop"),
        stream_of(TOOL_ACTION, 8, Duration::ZERO, "stop"),
        Reply::Json(json!({
            "model": "fake-model",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "Title"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 2}
        })),
    ])
    .await;
    let valve = start_valve(&upstream.base(), ValveMode::Constrained).await;

    let plain = post(&valve.url(), &chat_request(false)).await;
    assert_eq!(plain.status(), 200);
    let plain: Value = serde_json::from_str(&text(plain).await).unwrap();
    assert_eq!(
        plain["choices"][0]["message"]["content"],
        "It says: buy milk."
    );
    assert_eq!(plain["usage"]["prompt_tokens"], 100);

    let streamed = post(&valve.url(), &chat_request(true)).await;
    assert_eq!(streamed.status(), 200);
    assert!(text(streamed).await.ends_with("data: [DONE]\n\n"));

    let auxiliary =
        json!({"model": "local", "messages": [{"role": "user", "content": "Title this."}]});
    let auxiliary = post(&valve.url(), &auxiliary).await;
    assert_eq!(auxiliary.status(), 200);
    text(auxiliary).await;

    let receipts = receipts_eventually(&valve, 3).await;
    assert_eq!(receipts.len(), 3);
    let modes: Vec<_> = receipts
        .iter()
        .map(|r| r["mode"].as_str().unwrap())
        .collect();
    assert_eq!(modes, ["constrained", "constrained", "pass-through"]);
    assert!(receipts.iter().all(|r| r["outcome"] == "completed"));
    assert_eq!(receipts[0]["tool"], "task_complete");
    assert_eq!(receipts[1]["tool"], "read_file");
    assert_eq!(receipts[1]["finish_reason"], "tool_calls");
    assert_eq!(receipts[0]["cached_tokens"], 90);
    assert_eq!(receipts[0]["tools_offered"], 1);
    assert!(receipts[0]["schema_hash"].is_string());
    assert_eq!(receipts[2]["prompt_tokens"], 12);
    assert!(
        receipts[2]["unavailable"]
            .as_array()
            .unwrap()
            .contains(&json!("cached_tokens"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn receipt_contains_no_message_text() {
    const SECRET: &str = "PINEAPPLE-7731";
    let action = format!(
        r#"{{"thought":"{SECRET} thought","tool":"task_complete","args":{{"summary":"{SECRET} reply"}}}}"#
    );
    let upstream = FakeUpstream::start(vec![stream_of(&action, 7, Duration::ZERO, "stop")]).await;
    let valve = start_valve(&upstream.base(), ValveMode::Constrained).await;
    let mut body = chat_request(true);
    body["messages"][1]["content"] = json!(format!("Remember {SECRET}"));
    let response = post(&valve.url(), &body).await;
    assert!(
        text(response).await.contains(SECRET),
        "the client does receive the reply"
    );
    receipts_eventually(&valve, 1).await;
    let raw = std::fs::read_to_string(&valve.receipts).unwrap();
    assert!(
        !raw.contains(SECRET),
        "receipts must be content-free: {raw}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_request_writes_cancelled_receipt() {
    let mut frames = vec![(Duration::ZERO, content_frame(r#"{"thought":""#))];
    for _ in 0..200 {
        frames.push((Duration::from_millis(50), content_frame("x")));
    }
    let upstream = FakeUpstream::start(vec![Reply::Sse(frames)]).await;
    let valve = start_valve(&upstream.base(), ValveMode::Constrained).await;
    let mut response = post(&valve.url(), &chat_request(true)).await;
    assert_eq!(response.status(), 200);
    tokio::time::timeout(LIMIT, response.chunk())
        .await
        .unwrap()
        .unwrap();
    drop(response);
    let receipts = receipts_eventually(&valve, 1).await;
    assert_eq!(receipts[0]["outcome"], "cancelled");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !upstream.abandoned() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "upstream never saw the close"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_accepts_enforcing_upstream() {
    let upstream = FakeUpstream::start(vec![Reply::Json(json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "{\"ok\": \"yes\"}"}, "finish_reason": "stop"}]
    }))])
    .await;
    probe_enforcement(&upstream_client().unwrap(), &upstream.base())
        .await
        .unwrap();
    let sent = &upstream.requests()[0];
    assert_eq!(
        sent["response_format"]["json_schema"]["schema"]["properties"]["ok"]["enum"],
        json!(["yes"])
    );
    assert!(
        sent["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("hello")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_refuses_non_enforcing_upstream() {
    let upstream = FakeUpstream::start(vec![Reply::Json(json!({
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}]
    }))])
    .await;
    assert_eq!(
        probe_enforcement(&upstream_client().unwrap(), &upstream.base())
            .await
            .unwrap_err(),
        ProbeError::NotEnforced("hello".to_string())
    );
    let failing = FakeUpstream::start(vec![Reply::Status(500, "boom".to_string())]).await;
    assert!(matches!(
        probe_enforcement(&upstream_client().unwrap(), &failing.base()).await,
        Err(ProbeError::Http { status: 500, .. })
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn record_only_is_byte_faithful_streaming() {
    let native = Reply::Sse(vec![
        (Duration::ZERO, "data: {\"model\":\"fake-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n".to_string()),
        (Duration::ZERO, "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"a\\\"}\"}}]},\"finish_reason\":null}]}\n\n".to_string()),
        (Duration::ZERO, common::finish_frame("tool_calls")),
        (Duration::ZERO, common::done_frame()),
    ]);
    let upstream = FakeUpstream::start(vec![native]).await;
    let valve = start_valve(&upstream.base(), ValveMode::RecordOnly).await;
    let body = chat_request(true);
    let direct = text(post(&upstream.url(), &body).await).await;
    let through = text(post(&valve.url(), &body).await).await;
    assert_eq!(
        through, direct,
        "record-only must forward the stream byte-for-byte"
    );
    let requests = upstream.requests();
    assert_eq!(
        requests[0], requests[1],
        "the upstream saw the identical request"
    );
    assert!(requests[1].get("tools").is_some());
    assert!(requests[1].get("response_format").is_none());
    let receipts = receipts_eventually(&valve, 1).await;
    assert_eq!(receipts[0]["mode"], "record-only");
    assert_eq!(receipts[0]["tool"], "read_file");
    assert_eq!(receipts[0]["cached_tokens"], 90);
    assert_eq!(receipts[0]["finish_reason"], "tool_calls");
}

#[tokio::test(flavor = "multi_thread")]
async fn record_only_is_byte_faithful_nonstream() {
    let reply = json!({
        "model": "fake-model",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "timings": {"prompt_n": 3, "cache_n": 7, "predicted_n": 1, "prompt_ms": 1.0, "predicted_ms": 2.0}
    });
    let upstream = FakeUpstream::start(vec![Reply::Json(reply)]).await;
    let valve = start_valve(&upstream.base(), ValveMode::RecordOnly).await;
    let body = chat_request(false);
    let direct = text(post(&upstream.url(), &body).await).await;
    let through = text(post(&valve.url(), &body).await).await;
    assert_eq!(through, direct);
    let receipts = receipts_eventually(&valve, 1).await;
    assert_eq!(receipts[0]["mode"], "record-only");
    assert_eq!(receipts[0]["prompt_tokens"], 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn hermes_captured_request_round_trips() {
    // Real Hermes `file` toolset definitions, captured from Amalgam.
    let tools = hermes_tools();
    let action = r#"{"thought":"find the notes","tool":"search_files","args":{"pattern":"notes*","target":"files"}}"#;
    let upstream = FakeUpstream::start(vec![stream_of(action, 5, Duration::ZERO, "stop")]).await;
    let valve = start_valve(&upstream.base(), ValveMode::Constrained).await;
    let body = json!({
        "model": "amalgam-pilot",
        "stream": true,
        "max_tokens": 512,
        "messages": [
            {"role": "system", "content": "You are Hermes Agent."},
            {"role": "user", "content": "Find my notes file."}
        ],
        "tools": tools,
        "chat_template_kwargs": {"enable_thinking": false}
    });
    let streamed = text(post(&valve.url(), &body).await).await;

    let sent = &upstream.requests()[0];
    assert!(sent.get("tools").is_none());
    assert_eq!(
        sent["chat_template_kwargs"],
        json!({"enable_thinking": false})
    );
    let branches: Vec<_> = sent["response_format"]["json_schema"]["schema"]["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            b["properties"]["tool"]["const"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        branches,
        [
            "patch",
            "read_file",
            "search_files",
            "write_file",
            "task_complete"
        ]
    );
    let system = sent["messages"][0]["content"].as_str().unwrap();
    for name in [
        "patch",
        "read_file",
        "search_files",
        "write_file",
        "task_complete",
    ] {
        assert!(
            system.contains(&format!("\n- {name}: ")),
            "listing names {name}"
        );
    }

    let mut arguments = String::new();
    let mut name = String::new();
    for line in streamed.lines().filter_map(|l| l.strip_prefix("data: ")) {
        if line == "[DONE]" {
            continue;
        }
        let chunk: Value = serde_json::from_str(line).unwrap();
        if let Some(call) = chunk.pointer("/choices/0/delta/tool_calls/0") {
            if let Some(n) = call.pointer("/function/name").and_then(Value::as_str) {
                name.push_str(n);
            }
            if let Some(a) = call.pointer("/function/arguments").and_then(Value::as_str) {
                arguments.push_str(a);
            }
        }
    }
    assert_eq!(name, "search_files");
    assert_eq!(
        serde_json::from_str::<Value>(&arguments).unwrap(),
        json!({"pattern": "notes*", "target": "files"})
    );
    let receipts = receipts_eventually(&valve, 1).await;
    assert_eq!(receipts[0]["tools_offered"], 4);
}
