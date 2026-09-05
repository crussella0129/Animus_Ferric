use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::*;

#[derive(Clone, Copy)]
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
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<(String, bool)>>,
}

impl HttpFixture {
    fn start(response: Response) -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let location = format!("{base}/must-not-follow");
        let accepted = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_accepted = Arc::clone(&accepted);
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
                .set_write_timeout(Some(Duration::from_millis(200)))
                .unwrap();
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
            if stream.write_all(headers.as_bytes()).is_err() {
                return (request, true);
            }
            if matches!(response, Response::ChunkedLarge) {
                let chunk = vec![b' '; 16 * 1024];
                for _ in 0..=64 {
                    if stream
                        .write_all(b"4000\r\n")
                        .and_then(|_| stream.write_all(&chunk))
                        .and_then(|_| stream.write_all(b"\r\n"))
                        .is_err()
                    {
                        return (request, true);
                    }
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            }
            if matches!(response, Response::Ready | Response::Redirect) {
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
            stop,
            worker: Some(worker),
        }
    }

    fn finish(mut self) -> (String, bool) {
        self.worker.take().unwrap().join().unwrap()
    }
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
        assert!(result.is_err());
        let (request, closed) = fixture.finish();
        assert!(request.starts_with("GET /v1/models "));
        assert!(closed);
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
