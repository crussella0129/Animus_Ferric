use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::*;

#[derive(Clone, Copy, Debug)]
enum Response {
    Ready,
    Redirect,
    DeclaredLarge,
    ChunkedLarge,
    HeadersStall,
    BodyStall,
}

struct HttpFixture {
    base: String,
    accepted: Arc<AtomicBool>,
    redirected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    write_diagnostic: Arc<Mutex<String>>,
    worker: Option<JoinHandle<(String, bool)>>,
}

impl HttpFixture {
    fn start(response: Response) -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let location = format!("{base}/must-not-follow");
        let accepted = Arc::new(AtomicBool::new(false));
        let redirected = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let write_diagnostic = Arc::new(Mutex::new("response write not started".into()));
        let worker_diagnostic = Arc::clone(&write_diagnostic);
        let worker_accepted = Arc::clone(&accepted);
        let worker_redirected = Arc::clone(&redirected);
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(7);
            let mut stream = loop {
                if Instant::now() >= deadline || worker_stop.load(Ordering::Relaxed) {
                    return (String::new(), false);
                }
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => return (String::new(), false),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            stream.set_nodelay(true).unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                if bytes.len() > 8192
                    || Instant::now() >= deadline
                    || worker_stop.load(Ordering::Relaxed)
                {
                    return (String::new(), false);
                }
                match stream.read(&mut buffer) {
                    Ok(0) => return (String::new(), true),
                    Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => return (String::new(), true),
                }
            }
            worker_accepted.store(true, Ordering::Relaxed);
            let request = String::from_utf8(bytes).unwrap();
            let headers = match response {
                Response::Ready => "HTTP/1.1 200 OK\r\nContent-Length: 36\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"fixture-model-id\"}]}".to_string(),
                Response::Redirect => format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                Response::DeclaredLarge => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", BODY_LIMIT + 1),
                Response::ChunkedLarge => "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".into(),
                Response::HeadersStall => String::new(),
                Response::BodyStall => "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{".into(),
            };
            // Coalesce framing and body before writing. Separate tiny chunk
            // headers/trailers can induce delayed-ACK/backpressure stalls and
            // are not what the byte-limit assertion is trying to qualify.
            let mut wire = headers.into_bytes();
            if matches!(response, Response::ChunkedLarge) {
                let chunk = vec![b' '; 16 * 1024];
                for _ in 0..=64 {
                    wire.extend_from_slice(b"4000\r\n");
                    wire.extend_from_slice(&chunk);
                    wire.extend_from_slice(b"\r\n");
                }
                wire.extend_from_slice(b"0\r\n\r\n");
            }
            let write_deadline = deadline.min(Instant::now() + Duration::from_secs(3));
            let outcome = write_response_bounded(&mut stream, &wire, write_deadline, &worker_stop);
            *worker_diagnostic.lock().unwrap() = match &outcome {
                Ok(written) => format!(
                    "{response:?}: response write completed {written}/{} bytes",
                    wire.len()
                ),
                Err((written, kind)) => format!(
                    "{response:?}: response write stopped {written}/{} bytes at {kind:?}",
                    wire.len()
                ),
            };
            if let Err((_, kind)) = outcome {
                return (
                    request,
                    matches!(
                        kind,
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::BrokenPipe
                    ),
                );
            }
            if matches!(response, Response::Redirect) {
                let _ = stream.shutdown(Shutdown::Both);
                let trap_deadline = Instant::now() + Duration::from_millis(250);
                while Instant::now() < trap_deadline {
                    match listener.accept() {
                        Ok((mut redirected, _)) => {
                            worker_redirected.store(true, Ordering::Relaxed);
                            redirected
                                .set_write_timeout(Some(Duration::from_millis(200)))
                                .unwrap();
                            let _ = redirected.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 36\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"fixture-model-id\"}]}");
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(_) => break,
                    }
                }
                return (request, true);
            }
            if matches!(response, Response::Ready) {
                let _ = stream.shutdown(Shutdown::Both);
                return (request, true);
            }
            // The fixture proves a timed-out/cancelled request closes/reset its
            // socket instead of abandoning an in-flight asynchronous operation.
            while Instant::now() < deadline && !worker_stop.load(Ordering::Relaxed) {
                match stream.read(&mut buffer) {
                    Ok(0) => return (request, true),
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => return (request, true),
                }
            }
            (request, false)
        });
        Self {
            base,
            accepted,
            redirected,
            stop,
            write_diagnostic,
            worker: Some(worker),
        }
    }

    fn finish(mut self) -> (String, bool) {
        self.worker.take().unwrap().join().unwrap()
    }
}

fn write_response_bounded(
    stream: &mut impl Write,
    bytes: &[u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<usize, (usize, std::io::ErrorKind)> {
    let mut written = 0;
    while written < bytes.len() {
        if stop.load(Ordering::Relaxed) {
            return Err((written, std::io::ErrorKind::Interrupted));
        }
        if Instant::now() >= deadline {
            return Err((written, std::io::ErrorKind::TimedOut));
        }
        match stream.write(&bytes[written..]) {
            Ok(0) => return Err((written, std::io::ErrorKind::WriteZero)),
            Ok(count) => written += count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(error) => return Err((written, error.kind())),
        }
    }
    if stop.load(Ordering::Relaxed) {
        return Err((written, std::io::ErrorKind::Interrupted));
    }
    if Instant::now() >= deadline {
        return Err((written, std::io::ErrorKind::TimedOut));
    }
    Ok(written)
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

#[test]
fn startup_probe_limits_streamed_bodies_and_redirects() {
    for response in [
        Response::DeclaredLarge,
        Response::ChunkedLarge,
        Response::Redirect,
    ] {
        let fixture = HttpFixture::start(response);
        let result = models(
            &fixture.base,
            "fixture-key",
            &AtomicBool::new(false),
            Instant::now() + PROBE_TIMEOUT,
        );
        let redirected = Arc::clone(&fixture.redirected);
        let diagnostic = Arc::clone(&fixture.write_diagnostic);
        let (request, closed) = fixture.finish();
        let diagnostic = diagnostic.lock().unwrap();
        let error = result.expect_err("oversized/redirected metadata must not be admitted");
        match response {
            Response::DeclaredLarge | Response::ChunkedLarge => assert!(
                error.to_string().contains("exceeds one MiB"),
                "unexpected oversized-body failure: {error}; {diagnostic}"
            ),
            Response::Redirect => assert!(
                error
                    .to_string()
                    .contains("rejected the probe or redirected it"),
                "unexpected redirect failure: {error}; {diagnostic}"
            ),
            _ => unreachable!(),
        }
        assert!(request.starts_with("GET /v1/models "));
        assert!(
            closed,
            "fixture did not observe client closure: {diagnostic}"
        );
        assert!(
            !redirected.load(Ordering::Relaxed),
            "probe followed the redirect target"
        );
    }
}

#[test]
fn startup_credentials_stay_endpoint_bound() {
    let fixture = HttpFixture::start(Response::Ready);
    let ids = models(
        &fixture.base,
        "explicit-only-fixture-key",
        &AtomicBool::new(false),
        Instant::now() + PROBE_TIMEOUT,
    )
    .unwrap();
    let (request, _) = fixture.finish();
    assert_eq!(ids, ["fixture-model-id"]);
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer explicit-only-fixture-key\r\n")
    );
}

#[test]
fn startup_probe_cancellation_closes_headers_and_body() {
    for response in [Response::HeadersStall, Response::BodyStall] {
        let fixture = HttpFixture::start(response);
        let accepted = Arc::clone(&fixture.accepted);
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancel);
        let trigger = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(1);
            while !accepted.load(Ordering::Relaxed) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            signal.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let error = models(
            &fixture.base,
            "ferric-local",
            &cancel,
            Instant::now() + PROBE_TIMEOUT,
        )
        .unwrap_err();
        trigger.join().unwrap();
        let (_, closed) = fixture.finish();
        assert!(error.is_cancelled());
        assert!(closed);
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn startup_probe_deadlines_are_finite() {
    let fixture = HttpFixture::start(Response::BodyStall);
    let start = Instant::now();
    let result = models(
        &fixture.base,
        "ferric-local",
        &AtomicBool::new(false),
        Instant::now() + Duration::from_millis(150),
    );
    let (_, closed) = fixture.finish();
    assert!(result.is_err());
    assert!(closed);
    assert!(start.elapsed() < Duration::from_secs(2));
    let fixture = HttpFixture::start(Response::HeadersStall);
    let start = Instant::now();
    let result = models(
        &fixture.base,
        "ferric-local",
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(180),
    );
    let (_, closed) = fixture.finish();
    assert!(result.is_err());
    assert!(closed);
    assert!(start.elapsed() < Duration::from_secs(6));
}

#[test]
fn startup_fixture_write_preserves_partial_progress_and_deadline() {
    struct InterruptedWriter {
        calls: usize,
        bytes: Vec<u8>,
        always_block: bool,
    }
    impl Write for InterruptedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.always_block || self.calls == 3 {
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            if self.calls == 2 {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            let count = if self.calls == 1 {
                4
            } else if self.calls == 4 {
                2
            } else {
                bytes.len()
            }
            .min(bytes.len());
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let wire = b"framing plus response body";
    let mut writer = InterruptedWriter {
        calls: 0,
        bytes: Vec::new(),
        always_block: false,
    };
    assert_eq!(
        write_response_bounded(
            &mut writer,
            wire,
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false)
        )
        .unwrap(),
        wire.len()
    );
    assert_eq!(
        writer.bytes, wire,
        "partial progress must not be dropped or replayed after a polling timeout"
    );
    assert_eq!(writer.calls, 5);
    let mut writer = InterruptedWriter {
        calls: 0,
        bytes: Vec::new(),
        always_block: true,
    };
    let started = Instant::now();
    let failure = write_response_bounded(
        &mut writer,
        wire,
        started + Duration::from_millis(20),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert_eq!(failure, (0, std::io::ErrorKind::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(1));

    struct LateFinalWrite;
    impl Write for LateFinalWrite {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(30));
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let late = write_response_bounded(
        &mut LateFinalWrite,
        wire,
        Instant::now() + Duration::from_millis(5),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert_eq!(
        late,
        (wire.len(), std::io::ErrorKind::TimedOut),
        "a final write observed after the deadline is not an on-time success"
    );
}
