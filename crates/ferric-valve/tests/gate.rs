//! Sprint 127 (INT-0015): the valve behind the network gate. The gate token
//! is required, refusals never reach the upstream, and the upstream only
//! ever sees its own key, never the client's credential.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderMap, header};
use ferric_gate::{Gate, Token};
use ferric_valve::receipt::ReceiptSink;
use ferric_valve::{ValveConfig, ValveMode, gated_router, serve};
use serde_json::{Value, json};

/// An upstream that records the Authorization header of every request.
async fn echo_upstream() -> (String, Arc<Mutex<Vec<Option<String>>>>) {
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let record = seen.clone();
    let app = Router::new().fallback(move |headers: HeaderMap| {
        let record = record.clone();
        async move {
            let auth = headers
                .get(header::AUTHORIZATION)
                .map(|v| v.to_str().unwrap().to_string());
            record.lock().unwrap().push(auth);
            axum::Json(json!({"object": "chat.completion", "choices": []}))
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, seen)
}

async fn start(upstream: &str, token: &Token, upstream_key: Option<&str>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = gated_router(
        ValveConfig {
            upstream: upstream.to_string(),
            mode: ValveMode::RecordOnly,
            heartbeat: Duration::from_secs(2),
        },
        ReceiptSink::disabled(),
        Gate::new(token, addr.port()),
        upstream_key,
    )
    .unwrap();
    tokio::spawn(async move { serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn chat_body() -> Value {
    json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]})
}

#[tokio::test]
async fn refusals_never_reach_the_upstream() {
    let (upstream, seen) = echo_upstream().await;
    let token = Token::generate().unwrap();
    let valve = start(&upstream, &token, None).await;
    let client = reqwest::Client::new();

    let no_token = client
        .post(format!("{valve}/v1/chat/completions"))
        .json(&chat_body())
        .send()
        .await
        .unwrap();
    assert_eq!(no_token.status(), 401);

    // The pass-through fallback (llama.cpp's /slots, /props, ...) is gated too.
    let fallback = client.get(format!("{valve}/slots")).send().await.unwrap();
    assert_eq!(fallback.status(), 401);

    let drive_by = client
        .post(format!("{valve}/v1/chat/completions"))
        .header("origin", "https://attacker.example")
        .header("content-type", "text/plain")
        .body(chat_body().to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(drive_by.status(), 403);

    assert!(seen.lock().unwrap().is_empty(), "upstream was reached");
}

#[tokio::test]
async fn the_upstream_sees_its_own_key_and_never_the_gate_token() {
    let (upstream, seen) = echo_upstream().await;
    let token = Token::generate().unwrap();
    let valve = start(&upstream, &token, Some("upstream-secret")).await;
    let response = reqwest::Client::new()
        .post(format!("{valve}/v1/chat/completions"))
        .bearer_auth(token.as_str())
        .json(&chat_body())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, vec![Some("Bearer upstream-secret".to_string())]);
}

#[tokio::test]
async fn without_an_upstream_key_nothing_is_forwarded() {
    let (upstream, seen) = echo_upstream().await;
    let token = Token::generate().unwrap();
    let valve = start(&upstream, &token, None).await;
    let response = reqwest::Client::new()
        .post(format!("{valve}/v1/chat/completions"))
        .bearer_auth(token.as_str())
        .json(&chat_body())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(seen.lock().unwrap().clone(), vec![None]);
}
