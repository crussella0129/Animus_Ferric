//! T-12101: actual Cargo-built query requests, not policy-only propagation.
#![cfg(feature = "backend-openai")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::{Value, json};

mod support;
use support::FakeOpenAi;

/// Every request completes the task on one main action, streamed (SSE) when
/// the client asked to stream.
fn server() -> FakeOpenAi {
    FakeOpenAi::new(|_, request| {
        let action = json!({"thought":"complete", "tool":"task_complete","args":{"summary":"budget fixture complete"}});
        if !request["stream"].as_bool().unwrap_or(false) {
            return support::completion(&action, "stop");
        }
        let event = json!({"choices":[{"index":0,"delta":{"content":action.to_string()},"finish_reason":null}]});
        let end = json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":10}});
        Some((
            "200 OK",
            "text/event-stream",
            format!("data: {event}\n\ndata: {end}\n\ndata: [DONE]\n\n"),
        ))
    })
}

fn query(workspace: &Path, endpoint: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferric"));
    command
        .current_dir(workspace)
        .args([
            "query",
            "complete the fixture",
            "--no-config",
            "--model",
            "budget-fixture",
            "--api-base",
            endpoint,
            "--protocol",
            "grammar",
            "--params-b",
            "7",
            "--max-turns",
            "2",
            "--temperature",
            "0",
        ])
        .arg("--workspace")
        .arg(workspace)
        .arg("--profile-dir")
        .arg(workspace.join("unused-profiles"))
        .env_remove("OPENAI_API_KEY")
        .env_remove("FERRIC_PROMPTS_DIR")
        .env_remove("FERRIC_LOG")
        .env_remove("RUST_LOG");
    command
}

fn output(command: &mut Command) -> Output {
    support::output(command, Duration::from_secs(30))
}

fn events(workspace: &Path) -> Vec<Value> {
    let directory = workspace.join(".ferric/trace");
    let paths: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 1);
    std::fs::read_to_string(&paths[0])
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["event"].clone())
        .collect()
}

#[test]
fn query_output_budget_rejects_before_effects() {
    let mut server = server();
    for (ctx, cap) in [
        ("4096", "0"),
        ("4096", "-1"),
        ("4096", "4294967296"),
        ("4096", "1230"),
        ("0", "1"),
    ] {
        let workspace = tempfile::tempdir().unwrap();
        let result = output(query(workspace.path(), &server.endpoint).args([
            "--ctx",
            ctx,
            "--max-output-tokens",
            cap,
        ]));
        assert!(!result.status.success(), "ctx={ctx} cap={cap}");
        assert_eq!(
            std::fs::read_dir(workspace.path()).unwrap().count(),
            0,
            "invalid budget caused workspace effects"
        );
        assert!(
            server.requests.lock().unwrap().is_empty(),
            "invalid budget contacted provider"
        );
    }
    server.finish();
}

#[test]
fn http_budget_fixture_waits_for_fragmented_request() {
    let mut server = server();
    let address = server
        .endpoint
        .strip_prefix("http://")
        .unwrap()
        .strip_suffix("/v1")
        .unwrap();
    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    server
        .accepted
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    // The accepted connection deliberately has no request bytes yet. On
    // Windows an inherited nonblocking socket would fail immediately here.
    std::thread::sleep(Duration::from_millis(50));
    let body = json!({"stream":false,"max_tokens":1024}).to_string();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    for fragment in request.as_bytes().chunks(7) {
        client.write_all(fragment).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.finish();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(
        server.requests.lock().unwrap().as_slice(),
        [json!({"stream":false,"max_tokens":1024})]
    );
}

#[test]
fn query_output_budget_request_trace_wire_agree() {
    for streaming in [false, true] {
        let mut server = server();
        let workspace = tempfile::tempdir().unwrap();
        let mut command = query(workspace.path(), &server.endpoint);
        command.args(["--ctx", "4096", "--max-output-tokens", "1024"]);
        if !streaming {
            command.arg("--no-stream");
        }
        let result = output(&mut command);
        server.finish();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            1,
            "fixture must complete on one main action"
        );
        assert_eq!(requests[0]["max_tokens"], 1024);
        assert_eq!(requests[0]["stream"].as_bool().unwrap_or(false), streaming);
        let events = events(workspace.path());
        let policy = events
            .iter()
            .find(|event| event["type"] == "policy_selected")
            .unwrap();
        assert_eq!(policy["max_output_tokens"], 1024);
        let observed: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "main_action_budget")
            .collect();
        assert_eq!(observed.len(), 1);
        assert_eq!(
            observed[0]["budget"],
            json!({"requested":1024,"effective":1024,"declared_ctx":4096,"source":"explicit"})
        );
        assert_eq!(policy["tier_source"], "params");
    }
}
