//! Provider I/O acceptance uses joined, deadline-bounded futures. No fixture
//! task is detached, including when a provider assertion or timeout fails.

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::*;
use crate::types::SamplingParams;

const FIXTURE_DEADLINE: Duration = Duration::from_secs(4);
const CANCELLATION_DEADLINE: Duration = Duration::from_secs(2);

fn request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message::user("hello")],
        sampling: SamplingParams::default(),
        tools: Vec::new(),
        constraint: None,
    }
}

fn provider(address: std::net::SocketAddr) -> OpenAiProvider {
    OpenAiProvider::new(OpenAiConfig {
        base_url: format!("http://{address}/v1"),
        api_key: "fixture-key".to_string(),
        model: "fixture-model".to_string(),
    })
}

fn decode_chunks(
    constrained: bool,
    chunks: &[&[u8]],
) -> Result<(Completion, Vec<StreamDelta>), ProviderError> {
    let deltas = Mutex::new(Vec::new());
    let sink = |delta| deltas.lock().unwrap().push(delta);
    let mut acc = StreamAccumulator::new(constrained, &sink);
    let mut decoder = SseDecoder::default();
    for chunk in chunks {
        for line in decoder.push(chunk)? {
            if let SseLine::Data(data) = line {
                acc.feed_line(&data);
            }
        }
    }
    decoder.finish()?;
    Ok((acc.finish(), deltas.into_inner().unwrap()))
}

fn event(value: serde_json::Value) -> String {
    format!("data: {value}\r\n\r\n")
}

#[test]
fn sse_unicode_every_split() {
    let prose = "café 🦀 日本語";
    let args = json!({"path": "日本語.txt", "content": prose});
    let action = json!({"tool": "task_complete", "args": {"summary": prose}}).to_string();
    let cases = [
        (
            false,
            event(json!({"choices": [{"delta": {"content": prose}, "finish_reason": "stop"}]})),
        ),
        (
            false,
            event(json!({"choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "call-1", "function": {
                    "name": "write_file", "arguments": args.to_string()
                }
            }]}, "finish_reason": "tool_calls"}]})),
        ),
        (
            true,
            event(json!({"choices": [{"delta": {"content": action}, "finish_reason": "stop"}]})),
        ),
    ];
    for (index, (constrained, event)) in cases.into_iter().enumerate() {
        let wire = format!("{event}data: [DONE]\r\n\r\n");
        let bytes = wire.as_bytes();
        let expected = decode_chunks(constrained, &[bytes]).unwrap();
        match index {
            0 => {
                assert_eq!(expected.0.message.text.as_deref(), Some(prose));
                assert_eq!(expected.1, vec![StreamDelta::Text(prose.to_string())]);
            }
            1 => {
                assert_eq!(expected.0.message.tool_calls.len(), 1);
                assert_eq!(expected.0.message.tool_calls[0].args, args);
            }
            2 => {
                assert_eq!(expected.0.message.text.as_deref(), Some(action.as_str()));
                assert!(expected.1.contains(&StreamDelta::Text(prose.to_string())));
            }
            _ => unreachable!(),
        }
        for split in 0..=bytes.len() {
            assert_eq!(
                decode_chunks(constrained, &[&bytes[..split], &bytes[split..]]).unwrap(),
                expected,
                "case {index}, byte split {split}"
            );
        }
        let single_bytes: Vec<_> = bytes.chunks(1).collect();
        assert_eq!(decode_chunks(constrained, &single_bytes).unwrap(), expected);
    }
}

#[test]
fn sse_malformed_utf8_reports_error() {
    for wire in [
        b"data: {\"choices\":[{\"delta\":{\"content\":\"\xff\"}}]}\n".as_slice(),
        b"data: {\"choices\":[{\"delta\":{\"content\":\"\xf0\x9f".as_slice(),
        b": malformed comment \xc0\xaf\n".as_slice(),
    ] {
        for split in 0..=wire.len() {
            let error = decode_chunks(false, &[&wire[..split], &wire[split..]])
                .expect_err("invalid bytes must never become replacement characters");
            assert!(
                matches!(error, ProviderError::Backend(ref message) if message.contains("UTF-8"))
            );
        }
    }
}

#[test]
fn sse_ascii_done_compatibility() {
    let wire = b": comment\r\nevent: completion\r\n\r\ndata: not json\n\
        data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\r\n\
        data: [DONE]\n\xff ignored after DONE\n";
    let (completion, deltas) = decode_chunks(false, &[wire]).unwrap();
    assert_eq!(completion.message.text.as_deref(), Some("ok"));
    assert_eq!(completion.input_tokens, Some(3));
    assert_eq!(completion.output_tokens, Some(2));
    assert!(completion.truncated);
    assert_eq!(deltas, vec![StreamDelta::Text("ok".to_string())]);
    let (completion, _) = decode_chunks(false, &[b"data: {\"unfinished\":true}"]).unwrap();
    assert!(completion.message.text.is_none());
}

/// Drain one full small HTTP request so a later read proves cancellation
/// closed the connection, rather than merely observing remaining request bytes.
async fn read_request(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2048];
    loop {
        let count = socket.read(&mut buffer).await?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended early",
            ));
        }
        request.extend_from_slice(&buffer[..count]);
        if request.len() > 65_536 {
            return Err(io::Error::other("fixture request exceeds its byte budget"));
        }
        if let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&request[..header_end]).map_err(io::Error::other)?;
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .ok_or_else(|| io::Error::other("fixture expected Content-Length"))?;
            if request.len() >= header_end + 4 + length {
                return Ok(request);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ResponsePhase {
    Headers,
    ErrorBody,
    JsonBody,
    StreamBody,
}

impl ResponsePhase {
    fn prefix(self) -> &'static [u8] {
        match self {
            Self::Headers => b"HTTP/1.1 200 OK\r\nContent-Type: ",
            Self::ErrorBody => b"HTTP/1.1 503 Unavailable\r\nContent-Length: 1000\r\n\r\nmodel ",
            Self::JsonBody => b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{\"choices\":",
            Self::StreamBody => b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"first\"}}]}\n\n",
        }
    }
}

#[tokio::test]
async fn provider_cancellation_all_response_phases() {
    for (streaming, phase) in [
        (false, ResponsePhase::Headers),
        (true, ResponsePhase::Headers),
        (false, ResponsePhase::ErrorBody),
        (true, ResponsePhase::ErrorBody),
        (false, ResponsePhase::JsonBody),
        (true, ResponsePhase::StreamBody),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = provider(listener.local_addr().unwrap());
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled_at = Mutex::new(None);
        let server = async {
            let (mut socket, _) = listener.accept().await?;
            let request = read_request(&mut socket).await?;
            socket.write_all(phase.prefix()).await?;
            socket.flush().await?;
            // Let the provider reach the deliberately incomplete response.
            tokio::time::sleep(Duration::from_millis(100)).await;
            *cancelled_at.lock().unwrap() = Some(Instant::now());
            cancel.store(true, Ordering::Relaxed);
            let mut byte = [0_u8; 1];
            match tokio::time::timeout(CANCELLATION_DEADLINE, socket.read(&mut byte)).await {
                Ok(Ok(0)) => {}
                Ok(Err(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                    ) => {}
                other => {
                    return Err(io::Error::other(format!(
                        "cancelled request did not close: {other:?}"
                    )));
                }
            }
            Ok::<_, io::Error>(request)
        };
        let client = async {
            let sink = |_: StreamDelta| {};
            let result = if streaming {
                provider
                    .complete_streaming(request(), &sink, Some(cancel.clone()))
                    .await
            } else {
                provider.complete(request(), Some(cancel.clone())).await
            };
            let elapsed = cancelled_at.lock().unwrap().map(|at| at.elapsed());
            (result, elapsed)
        };
        let (served, completed) = tokio::join!(
            tokio::time::timeout(FIXTURE_DEADLINE, server),
            tokio::time::timeout(FIXTURE_DEADLINE, client),
        );
        // Both owned futures have finished or been dropped by their deadline
        // before any assertion can unwind this test. No spawned task survives.
        let request = served
            .expect("server fixture deadline")
            .expect("request was closed after cancellation");
        let (result, elapsed) = completed.expect("provider fixture deadline");
        assert!(
            matches!(result, Err(ProviderError::Backend(ref message)) if message == "Interrupted"),
            "streaming={streaming}, phase={phase:?}: {result:?}"
        );
        assert!(
            elapsed.expect("cancellation was issued") < CANCELLATION_DEADLINE,
            "phase={phase:?}"
        );
        let request = String::from_utf8(request).unwrap();
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-key")
        );
        assert!(request.contains("\"model\":\"fixture-model\""));
    }
}

#[tokio::test]
async fn cancelled_provider_does_not_poll_request() {
    let polled = AtomicBool::new(false);
    let cancel = AtomicBool::new(true);
    let result = with_cancellation(Some(&cancel), async {
        polled.store(true, Ordering::Relaxed);
        Ok(())
    })
    .await;
    assert!(matches!(result, Err(ProviderError::Backend(ref message)) if message == "Interrupted"));
    assert!(!polled.load(Ordering::Relaxed));
}

#[tokio::test]
async fn sse_unicode_and_invalid_bytes_over_tcp() {
    let prose = "café 🦀 日本語";
    let valid = format!(
        "{}data: [DONE]\n\n",
        event(json!({"choices": [{"delta": {"content": prose}}]}))
    );
    for (wire, expected) in [
        (valid.into_bytes(), Some(prose)),
        (
            b"data: {\"choices\":[{\"delta\":{\"content\":\"\xff\"}}]}\n".to_vec(),
            None,
        ),
        (
            b"data: {\"choices\":[{\"delta\":{\"content\":\"\xf0\x9f".to_vec(),
            None,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = provider(listener.local_addr().unwrap());
        let server = async {
            let (mut socket, _) = listener.accept().await?;
            read_request(&mut socket).await?;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await?;
            for byte in wire {
                socket.write_all(&[byte]).await?;
                tokio::task::yield_now().await;
            }
            socket.shutdown().await
        };
        let sink = |_: StreamDelta| {};
        let (served, completed) = tokio::join!(
            tokio::time::timeout(FIXTURE_DEADLINE, server),
            tokio::time::timeout(
                FIXTURE_DEADLINE,
                provider.complete_streaming(request(), &sink, None)
            ),
        );
        served
            .expect("server fixture deadline")
            .expect("wire fixture completed");
        let completed = completed.expect("provider fixture deadline");
        if let Some(expected) = expected {
            assert_eq!(completed.unwrap().message.text.as_deref(), Some(expected));
        } else {
            assert!(
                matches!(completed, Err(ProviderError::Backend(ref message)) if message.contains("UTF-8"))
            );
        }
    }
}
