//! Shared helpers for the `ferric` binary's integration tests: one isolated
//! command builder, `--help` capture, and one scripted OpenAI-compatible
//! server. Each test file uses a subset.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Run `command` within `timeout`; every owned child is reaped first.
pub fn output(command: &mut Command, timeout: Duration) -> Output {
    ferric_process::testing::output(command, timeout)
        .expect("bounded command finished and every owned child was reaped")
}

/// `ferric` run in `workspace`, isolated from the user config layer and from
/// ambient keys, prompt libraries and log filters.
pub fn isolated_ferric(workspace: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferric"));
    command
        .current_dir(workspace)
        .env("APPDATA", workspace.join("unused-user-config"))
        .env("XDG_CONFIG_HOME", workspace.join("unused-user-config"))
        .env("HOME", workspace.join("unused-user-config"))
        .env_remove("OPENAI_API_KEY")
        .env_remove("FERRIC_PROMPTS_DIR")
        .env_remove("FERRIC_LOG")
        .env_remove("RUST_LOG");
    command
}

/// `ferric <arguments> --help` stdout. Help must succeed and must not prepare
/// resources or write session state in `workspace`.
pub fn help(workspace: &Path, arguments: &[&str]) -> String {
    let result = output(
        isolated_ferric(workspace).args(arguments).arg("--help"),
        Duration::from_secs(30),
    );
    assert!(
        result.status.success(),
        "help for {arguments:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_dir(workspace).unwrap().count(),
        0,
        "help must not prepare resources or write session state"
    );
    String::from_utf8(result.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

/// One scripted reply: `(status line, content type, body)`, or `None` to
/// stall the request until the server is finished.
pub type Reply = Option<(&'static str, &'static str, String)>;

/// A loopback OpenAI-compatible fixture. It records every request body and
/// answers request `n` with `respond(n, &body)`; it runs until `finish` or a
/// bounded deadline, and its worker is joined before the owner drops it.
pub struct FakeOpenAi {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    /// One message per accepted connection, before its request is read.
    pub accepted: mpsc::Receiver<()>,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl FakeOpenAi {
    pub fn new(respond: impl Fn(usize, &Value) -> Reply + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let (stop, stopped) = mpsc::channel();
        let (accepted_tx, accepted) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(45);
            while Instant::now() < deadline {
                if stopped.try_recv().is_ok() {
                    return;
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Winsock accept inherits listener nonblocking mode.
                        // Deadline-based framing below needs a blocking stream.
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let _ = accepted_tx.send(());
                        let request = read_request(&mut stream);
                        let index = {
                            let mut requests = captured.lock().unwrap();
                            requests.push(request.clone());
                            requests.len() - 1
                        };
                        let Some((status, content_type, body)) = respond(index, &request) else {
                            // A finite stalled request, released promptly by
                            // its owner once the client's reaping is checked.
                            let _ = stopped.recv_timeout(Duration::from_secs(20));
                            return;
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        stream.write_all(response.as_bytes()).unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if stopped.recv_timeout(Duration::from_millis(5)).is_ok() {
                            return;
                        }
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            }
        });
        Self {
            endpoint,
            requests,
            accepted,
            stop: Some(stop),
            worker: Some(worker),
        }
    }

    pub fn finish(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let deadline = Instant::now() + Duration::from_secs(6);
            while !worker.is_finished() {
                if Instant::now() >= deadline {
                    ferric_process::abort_on_cleanup_failure(
                        "HTTP fixture join",
                        "join deadline exceeded",
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            // Joining proved termination. A worker assertion/IO panic is a
            // fixture failure, not an unproved-lifetime failure.
            if let Err(payload) = worker.join()
                && !std::thread::panicking()
            {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

impl Drop for FakeOpenAi {
    fn drop(&mut self) {
        self.finish();
    }
}

/// A `200 OK` JSON chat completion whose assistant content is `action`.
pub fn completion(action: &Value, finish_reason: &str) -> Reply {
    Some((
        "200 OK",
        "application/json",
        serde_json::json!({
            "choices":[{"index":0,"message":{"role":"assistant","content":action.to_string()},"finish_reason":finish_reason}],
            "usage":{"prompt_tokens":20,"completion_tokens":10}
        })
        .to_string(),
    ))
}

fn read_request(stream: &mut TcpStream) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        assert!(Instant::now() < deadline, "bounded HTTP framing deadline");
        assert!(bytes.len() <= 512 * 1024, "bounded fixture request size");
        let read = stream.read(&mut buffer).expect("fixture request bytes");
        assert!(read > 0, "request ended before its declared body");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(split) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header = std::str::from_utf8(&bytes[..split]).unwrap();
            assert!(header.starts_with("POST /v1/chat/completions "));
            let length: usize = header
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .expect("content length");
            assert!(length <= 512 * 1024);
            if bytes.len() >= split + 4 + length {
                return serde_json::from_slice(&bytes[split + 4..split + 4 + length]).unwrap();
            }
        }
    }
}
