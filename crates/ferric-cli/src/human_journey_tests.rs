// Included inside human::enabled::tests so scripted IO and the exact session
// orchestration are shared, without exposing a fixture hook in product builds.

struct FixturePreparation {
    mode: &'static str,
    port: std::sync::atomic::AtomicU16,
}

impl FixturePreparation {
    fn new(mode: &'static str) -> Self {
        Self {
            mode,
            port: std::sync::atomic::AtomicU16::new(0),
        }
    }
    fn assert_closed(&self) {
        let port = self.port.load(Ordering::Acquire);
        if port != 0 {
            let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .expect("owned listener is closed after checked source-scope cleanup");
            drop(listener);
        }
    }
}

impl Preparation for FixturePreparation {
    fn begin(
        &self,
        root: &Path,
        cfg: &Config,
        model: Option<&Path>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Startup, crate::startup::StartupError> {
        crate::test_process_containment::ensure_current_process_tree_is_contained().unwrap();
        crate::startup::test_support::begin(root, cfg, model, cancel)
    }
    fn prepare(
        &self,
        start: Startup,
        index: usize,
        cancel: Arc<AtomicBool>,
        progress: &mut dyn FnMut(&str),
    ) -> Result<PreparedSession, crate::startup::StartupError> {
        crate::startup::test_support::prepare(start, index, cancel, progress, |port| {
            self.port.store(port, Ordering::Release);
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "human::enabled::tests::fixture_human_engine",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("FERRIC_HUMAN_FIXTURE", self.mode)
                .env("FERRIC_HUMAN_FIXTURE_PORT", port.to_string());
            command
        })
    }
}

fn fixture_models(root: &Path, count: usize) {
    std::fs::create_dir(root.join("models")).unwrap();
    for index in 0..count {
        let mut header = [0_u8; 24];
        header[..4].copy_from_slice(b"GGUF");
        header[4..8].copy_from_slice(&3_u32.to_le_bytes());
        std::fs::write(
            root.join("models").join(format!("fixture-{index}.gguf")),
            header,
        )
        .unwrap();
    }
}

fn fixture_read_request(
    socket: &mut std::net::TcpStream,
    limit: Duration,
) -> Option<(Vec<u8>, usize)> {
    use std::io::Read;
    let deadline = std::time::Instant::now() + limit;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            eprintln!(
                "fixture request deadline expired: retained_bytes={}, limit_ms={}",
                bytes.len(),
                limit.as_millis()
            );
            return None;
        };
        if bytes.len() > 256 * 1024 {
            return None;
        }
        socket
            .set_read_timeout(Some(remaining.min(Duration::from_millis(300))))
            .ok()?;
        let count = match socket.read(&mut buffer) {
            Ok(0) => return None,
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => {
                eprintln!(
                    "fixture request read failed: kind={:?}, retained_bytes={}",
                    error.kind(),
                    bytes.len()
                );
                return None;
            }
        };
        if std::time::Instant::now() >= deadline
            || count > (256 * 1024usize).saturating_sub(bytes.len())
        {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.windows(4).position(|slice| slice == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if length > 256 * 1024 {
                return None;
            }
            if bytes.len() >= end + 4 + length {
                return Some((bytes, end + 4));
            }
        }
    }
}

#[test]
fn fixture_request_poll_timeout_preserves_fragments_and_absolute_bound() {
    use std::io::Write;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut client = std::net::TcpStream::connect_timeout(
        &listener.local_addr().unwrap(),
        Duration::from_secs(2),
    )
    .unwrap();
    let (mut server, _) = listener.accept().unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 2\r\n\r\n{")
        .unwrap();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(450));
        client.write_all(b"}").unwrap();
    });
    let request = fixture_read_request(&mut server, Duration::from_secs(3));
    writer.join().unwrap();
    let (bytes, body_start) =
        request.expect("a polling timeout must not discard a partial request");
    assert_eq!(&bytes[body_start..], b"{}");
    let mut stalled = std::net::TcpStream::connect_timeout(
        &listener.local_addr().unwrap(),
        Duration::from_secs(2),
    )
    .unwrap();
    let (mut server, _) = listener.accept().unwrap();
    stalled
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stalled.write_all(b"GET /health ").unwrap();
    let started = std::time::Instant::now();
    assert!(fixture_read_request(&mut server, Duration::from_millis(600)).is_none());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn fixture_human_engine() {
    use std::io::{Read, Write};
    let Ok(mode) = std::env::var("FERRIC_HUMAN_FIXTURE") else {
        return;
    };
    let port: u16 = std::env::var("FERRIC_HUMAN_FIXTURE_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    let mut work_turn = 0;
    while std::time::Instant::now() < deadline {
        let mut socket = match listener.accept() {
            Ok((socket, _)) => socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => break,
        };
        socket
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let mut buffer = [0_u8; 4096];
        let Some((bytes, body_start)) = fixture_read_request(&mut socket, Duration::from_secs(3))
        else {
            continue;
        };
        let is_completion = bytes.starts_with(b"POST /v1/chat/completions ");
        let body = if bytes.starts_with(b"GET /health ") {
            "{}".to_string()
        } else if !is_completion {
            r#"{"data":[{"id":"human-fixture-model"}]}"#.into()
        } else {
            let request: serde_json::Value = serde_json::from_slice(&bytes[body_start..]).unwrap();
            if mode == "cancel" {
                let event = serde_json::json!({"choices":[{"delta":{"content":"pending"}}]});
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {event}\n\n"
                );
                let _ = socket.write_all(response.as_bytes());
                while std::time::Instant::now() < deadline {
                    match socket.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                            ) => {}
                        Err(_) => break,
                    }
                }
                continue;
            }
            let controlled = request.get("response_format").is_some();
            let content = if controlled {
                let action = if work_turn == 0 {
                    serde_json::json!({"tool":"write_file","args":{"path":"human-result.txt","content":"fixture work"}})
                } else {
                    serde_json::json!({"tool":"task_complete","args":{"summary":"Fixture work complete."}})
                };
                work_turn += 1;
                action.to_string()
            } else {
                assert!(
                    request.get("tools").is_none(),
                    "ask must not send tool authority"
                );
                "Hello from the source-owned fixture.".into()
            };
            if request["stream"].as_bool() == Some(true) {
                let event = serde_json::json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]});
                let wire = format!("data: {event}\n\ndata: [DONE]\n\n");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{wire}",
                    wire.len()
                );
                let _ = socket.write_all(response.as_bytes());
                continue;
            }
            serde_json::json!({"choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":10}}).to_string()
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes());
    }
}

fn run_fixture(
    root: &Path,
    lines: &[Option<&str>],
    preparation: &FixturePreparation,
) -> (Result<(), String>, ScriptedIo) {
    let io = ScriptedIo::new(lines);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = session_with(
        &RunArgs::default(),
        root,
        &Config::default(),
        true,
        &io,
        &runtime,
        Arc::new(AtomicBool::new(false)),
        preparation,
    );
    preparation.assert_closed();
    (result, io)
}

fn setup_decisions(io: &ScriptedIo) -> usize {
    io.prompts
        .lock()
        .unwrap()
        .iter()
        .filter(|prompt| !prompt.starts_with("You"))
        .count()
}

#[test]
fn human_first_run_decision_budget() {
    let root = tempfile::tempdir().unwrap();
    fixture_models(root.path(), 2);
    let preparation = FixturePreparation::new("ready");
    let (result, io) = run_fixture(
        root.path(),
        &[
            Some("2"),
            Some("ask"),
            Some("y"),
            Some("hello"),
            Some("/quit"),
        ],
        &preparation,
    );
    result.unwrap();
    assert_eq!(setup_decisions(&io), 3);
    let output = io.output.lock().unwrap();
    assert!(output.contains("Ready: human-fixture-model"));
    assert!(output.contains("Hello from the source-owned fixture."));
    assert!(!root.path().join("human-result.txt").exists());
    for technical in ["parameters?", "context?", "quantization?", "GPU layers?"] {
        assert!(
            !io.prompts
                .lock()
                .unwrap()
                .iter()
                .any(|prompt| prompt.contains(technical))
        );
    }
}

#[test]
fn human_repeat_reuses_model() {
    let root = tempfile::tempdir().unwrap();
    fixture_models(root.path(), 2);
    let first = FixturePreparation::new("ready");
    run_fixture(
        root.path(),
        &[Some("1"), Some("ask"), Some("y"), Some("/quit")],
        &first,
    )
    .0
    .unwrap();
    let second = FixturePreparation::new("ready");
    let (result, io) = run_fixture(
        root.path(),
        &[Some("ask"), Some("y"), Some("/quit")],
        &second,
    );
    result.unwrap();
    assert_eq!(setup_decisions(&io), 2);
    assert!(
        !io.prompts
            .lock()
            .unwrap()
            .iter()
            .any(|prompt| prompt.contains("Which model"))
    );
}

#[test]
fn human_stale_single_model_requires_reselection() {
    let root = tempfile::tempdir().unwrap();
    fixture_models(root.path(), 1);
    let preparation = FixturePreparation::new("ready");
    run_fixture(
        root.path(),
        &[Some("ask"), Some("y"), Some("/quit")],
        &preparation,
    )
    .0
    .unwrap();
    let model = root.path().join("models/fixture-0.gguf");
    let mut bytes = std::fs::read(&model).unwrap();
    bytes.push(0);
    std::fs::write(&model, bytes).unwrap();
    let next = FixturePreparation::new("ready");
    let (result, io) = run_fixture(
        root.path(),
        &[Some("1"), Some("ask"), Some("y"), Some("/quit")],
        &next,
    );
    result.unwrap();
    assert_eq!(setup_decisions(&io), 3);
    assert!(
        io.output
            .lock()
            .unwrap()
            .contains("saved model choice changed")
    );
}

#[test]
fn human_journey_e2e_matrix() {
    for lines in [vec![None], vec![Some("ask"), Some("n")], vec![Some("quit")]] {
        let root = tempfile::tempdir().unwrap();
        fixture_models(root.path(), 1);
        let preparation = FixturePreparation::new("ready");
        run_fixture(root.path(), &lines, &preparation).0.unwrap();
        assert_eq!(
            preparation.port.load(Ordering::Acquire),
            0,
            "decline/EOF must not start a process"
        );
        assert!(!root.path().join(".ferric/startup-preference.json").exists());
    }
    let absent = tempfile::tempdir().unwrap();
    let preparation = FixturePreparation::new("ready");
    let (result, _) = run_fixture(absent.path(), &[], &preparation);
    assert!(result.is_err());
    assert_eq!(preparation.port.load(Ordering::Acquire), 0);

    let root = tempfile::tempdir().unwrap();
    fixture_models(root.path(), 1);
    let preparation = FixturePreparation::new("ready");
    let (result, _) = run_fixture(
        root.path(),
        &[
            Some("work"),
            Some("y"),
            Some("Write human-result.txt with fixture work"),
            Some("/quit"),
        ],
        &preparation,
    );
    result.unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("human-result.txt")).unwrap(),
        "fixture work"
    );
    let trace = std::fs::read_dir(ferric_trace::trace_dir(root.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = std::fs::read_to_string(trace).unwrap();
    assert!(text.contains("\"harness_policy\":\"evidence\""));
    assert!(text.contains("\"tier_source\":\"conservative\""));
    assert!(text.contains("\"name\":\"write_file\""));
}

#[test]
fn human_cancel_during_request_reaps_owned_engine() {
    struct CancelIo {
        inner: ScriptedIo,
        cancel: Arc<AtomicBool>,
    }
    impl HumanIo for CancelIo {
        fn say(&self, text: &str) {
            self.inner.say(text);
        }
        fn delta(&self, text: &str) {
            self.inner.delta(text);
            self.cancel.store(true, Ordering::Release);
        }
        fn read(&self, prompt: &str) -> Result<Option<String>, String> {
            self.inner.read(prompt)
        }
    }
    let root = tempfile::tempdir().unwrap();
    fixture_models(root.path(), 1);
    let cancel = Arc::new(AtomicBool::new(false));
    let io = CancelIo {
        inner: ScriptedIo::new(&[Some("ask"), Some("y"), Some("hello")]),
        cancel: cancel.clone(),
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let preparation = FixturePreparation::new("cancel");
    let result = session_with(
        &RunArgs::default(),
        root.path(),
        &Config::default(),
        true,
        &io,
        &runtime,
        cancel,
        &preparation,
    );
    let error = result.unwrap_err();
    assert!(error.contains("Interrupted"), "{error}");
    preparation.assert_closed();
    let _again = crate::startup::test_support::begin(
        root.path(),
        &Config::default(),
        None,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
}

#[test]
#[ignore = "explicit live local-model acceptance; set FERRIC_LIVE_MODEL to an existing GGUF"]
fn real_model_prepared_host_journey() {
    crate::test_process_containment::ensure_current_process_tree_is_contained().unwrap();
    let model = std::path::PathBuf::from(
        std::env::var_os("FERRIC_LIVE_MODEL").expect("explicit existing local model"),
    );
    assert!(model.is_file());
    let root = tempfile::tempdir().unwrap();
    let args = RunArgs {
        model: Some(model.clone()),
        ..RunArgs::default()
    };
    let io = ScriptedIo::new(&[
        Some("ask"),
        Some("y"),
        Some("Reply with exactly: Ferric is ready."),
        Some("/quit"),
    ]);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let started = std::time::Instant::now();
    let result = session(
        &args,
        root.path(),
        &Config::default(),
        true,
        &io,
        &runtime,
        Arc::new(AtomicBool::new(false)),
    );
    let duration = started.elapsed();
    let traces: Vec<_> = std::fs::read_dir(ferric_trace::trace_dir(root.path()))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
        .collect();
    let evidence = serde_json::json!({
        "model_file": model, "elapsed_seconds": duration.as_secs_f64(), "context": 4096,
        "cpu_only": true, "qualification": "unmeasured prepared-host conversation only",
        "decisions_and_inputs": *io.answers.lock().unwrap(), "transcript": *io.output.lock().unwrap(), "traces": traces,
        "time_to_ready_seconds": io.ready_after.lock().unwrap().map(|time| time.as_secs_f64()),
        "time_to_first_response_seconds": io.response_after.lock().unwrap().map(|time| time.as_secs_f64()),
        "checked_cleanup": result.is_ok(), "result": result,
    });
    println!("LIVE_JOURNEY_EVIDENCE={evidence}");
    assert!(evidence["result"].get("Ok").is_some(), "{evidence}");
    assert!(
        io.output
            .lock()
            .unwrap()
            .contains("owned foreground (closed on exit)")
    );
    assert_eq!(setup_decisions(&io), 2);
    assert!(
        traces
            .iter()
            .any(|trace| trace.contains("\"reason\":\"answered\""))
    );
    // No retained startup lock owner survives successful session cleanup.
    let _again = crate::startup::test_support::begin(
        root.path(),
        &Config::default(),
        Some(&model),
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
}
