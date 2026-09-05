use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

use super::*;

fn scope(workspace: &Path) -> ManagedDiscoveryScope {
    ManagedDiscoveryScope {
        workspace: workspace.to_path_buf(),
        global: None,
    }
}

fn model_fixture(workspace: &Path) -> PathBuf {
    let directory = workspace.join("models");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("fixture.gguf");
    let mut header = [0_u8; 24];
    header[..4].copy_from_slice(b"GGUF");
    header[4..8].copy_from_slice(&3_u32.to_le_bytes());
    std::fs::write(&path, header).unwrap();
    path
}

fn begin_local(workspace: &Path) -> Startup {
    Startup::begin_in(
        workspace,
        &Config::default(),
        None,
        &AtomicBool::new(false),
        scope(workspace),
    )
    .unwrap()
}

fn fixture_command(mode: &str, port: u16) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "startup::tests::fixture_engine",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FERRIC_STARTUP_FIXTURE", mode)
        .env("FERRIC_STARTUP_FIXTURE_PORT", port.to_string());
    command
}

fn fixture_launch(
    mode: &'static str,
) -> impl FnOnce(&LocalModel, u32, u16, &AtomicBool, Instant) -> Result<(Command, String), StartupError>
{
    move |_, _, port, _, _| {
        Ok((
            fixture_command(mode, port),
            "source-defined native test engine".into(),
        ))
    }
}

/// Child mode exists only in this Cargo-built test source and self-expires.
/// Parent ProcessTree owners additionally kill/reap and join all diagnostics.
#[test]
fn fixture_engine() {
    let Ok(mode) = std::env::var("FERRIC_STARTUP_FIXTURE") else {
        return;
    };
    if mode == "version" {
        println!("fixture-engine 1");
        return;
    }
    if mode == "exit" {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    if mode == "absent" {
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        return;
    }
    let port: u16 = std::env::var("FERRIC_STARTUP_FIXTURE_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    listener.set_nonblocking(true).unwrap();
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_millis(200)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_millis(200)))
                    .unwrap();
                let mut request = [0_u8; 8192];
                let count = match stream.read(&mut request) {
                    Ok(count) => count,
                    Err(_) => continue,
                };
                let health = request[..count].starts_with(b"GET /health ");
                let (status, body) = if health && mode == "loading" {
                    (503, "loading")
                } else if health {
                    (200, "{}")
                } else if mode == "malformed" {
                    (200, "not json")
                } else {
                    (200, r#"{"data":[{"id":"actual-advertised-model"}]}"#)
                };
                let response = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.shutdown(Shutdown::Both);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(_) => break,
        }
    }
}

#[test]
fn startup_owned_runtime_reaches_ready() {
    let directory = tempfile::tempdir().unwrap();
    model_fixture(directory.path());
    let startup = begin_local(directory.path());
    assert_eq!(startup.models.len(), 1);
    assert!(startup.will_start_engine);
    let mut session = startup
        .prepare_with(
            0,
            &AtomicBool::new(false),
            &mut |_| {},
            fixture_launch("ready"),
        )
        .unwrap();
    assert_eq!(session.model, "actual-advertised-model");
    assert_eq!(session.context, 4096);
    assert_eq!(session.backend_opts.api_key.as_deref(), Some(LOCAL_KEY));
    session.validate().unwrap();
    assert!(!server::runfile_path(directory.path()).exists());
    let retained = match &session.ownership {
        Ownership::Owned(engine) => LiveProcess::acquire(engine.child.tree.child().id()).unwrap(),
        _ => unreachable!(),
    };
    let (trace, mut file) = session.create_trace_file("human-fixture.jsonl").unwrap();
    assert_eq!(
        trace.parent().unwrap(),
        ferric_trace::trace_dir(directory.path())
            .canonicalize()
            .unwrap()
    );
    file.write_all(b"{}\n").unwrap();
    assert!(trace.starts_with(directory.path().canonicalize().unwrap()));
    drop(file);
    session.cleanup().unwrap();
    assert!(retained.wait(Duration::from_secs(2)).unwrap());
    assert!(session.validate().is_err());
    drop(session);
    let startup = begin_local(directory.path());
    assert_eq!(startup.preferred_index, Some(0));
}

#[test]
fn startup_cleanup_fault_matrix() {
    for mode in ["exit", "malformed"] {
        let directory = tempfile::tempdir().unwrap();
        model_fixture(directory.path());
        let startup = begin_local(directory.path());
        assert!(
            startup
                .prepare_with(
                    0,
                    &AtomicBool::new(false),
                    &mut |_| {},
                    fixture_launch(mode)
                )
                .is_err()
        );
        // Preparation does not publish any preference on failed readiness.
        assert!(
            !directory
                .path()
                .join(".ferric/startup-preference.json")
                .exists()
        );
        let _again = begin_local(directory.path());
    }
    let directory = tempfile::tempdir().unwrap();
    model_fixture(directory.path());
    let session = begin_local(directory.path())
        .prepare_with(
            0,
            &AtomicBool::new(false),
            &mut |_| {},
            fixture_launch("ready"),
        )
        .unwrap();
    let retained = match &session.ownership {
        Ownership::Owned(engine) => LiveProcess::acquire(engine.child.tree.child().id()).unwrap(),
        _ => unreachable!(),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _session = session;
        panic!("injected session unwind");
    }));
    assert!(outcome.is_err());
    assert!(retained.wait(Duration::from_secs(2)).unwrap());
    let _again = begin_local(directory.path());
}

#[test]
fn startup_cancellation_reaps_scope() {
    for mode in ["absent", "loading"] {
        let port = free_port();
        let mut owned = OwnedEngine::spawn(fixture_command(mode, port), port).unwrap();
        let retained = LiveProcess::acquire(owned.child.tree.child().id()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancel);
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            signal.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let error = wait_ready(
            &mut owned,
            &format!("http://127.0.0.1:{port}/v1"),
            &cancel,
            Instant::now() + STARTUP_LIMIT,
        )
        .unwrap_err();
        trigger.join().unwrap();
        assert!(error.is_cancelled(), "mode {mode}: {error}");
        owned.child.cleanup().unwrap();
        assert!(retained.wait(Duration::from_secs(2)).unwrap());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn startup_listener_identity_mismatch_refuses() {
    let foreign = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = foreign.local_addr().unwrap().port();
    let mut owned = OwnedEngine::spawn(fixture_command("absent", port), port).unwrap();
    let retained = LiveProcess::acquire(owned.child.tree.child().id()).unwrap();
    assert!(
        wait_ready(
            &mut owned,
            &format!("http://127.0.0.1:{port}/v1"),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(2)
        )
        .is_err()
    );
    owned.child.cleanup().unwrap();
    assert!(retained.wait(Duration::from_secs(2)).unwrap());
    assert!(TcpStream::connect(foreign.local_addr().unwrap()).is_ok());
}

fn free_port() -> u16 {
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn ready_fixture() -> OwnedEngine {
    let port = free_port();
    let mut engine = OwnedEngine::spawn(fixture_command("ready", port), port).unwrap();
    wait_ready(
        &mut engine,
        &format!("http://127.0.0.1:{port}/v1"),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    engine
}

#[test]
fn startup_borrows_ready_server() {
    let directory = tempfile::tempdir().unwrap();
    let mut engine = ready_fixture();
    let runfile_path = server::runfile_path(directory.path());
    std::fs::create_dir_all(runfile_path.parent().unwrap()).unwrap();
    let runfile = server::ServerRunfile {
        schema_version: 2,
        engine: server::Engine::LlamaServer,
        pid: engine.child.tree.child().id(),
        port: engine.port,
        base_url: format!("http://127.0.0.1:{}/v1", engine.port),
        tailscale: false,
        tailscale_serve: None,
        model: Some("actual-advertised-model".into()),
        context_size: Some(4096),
        sampling_seed: None,
        parallel_slots: Some(1),
        process_identity: Some(engine.identity.clone()),
        origin_local_runfile: Some(runfile_path.clone()),
    };
    let bytes = serde_json::to_vec(&runfile).unwrap();
    std::fs::write(&runfile_path, &bytes).unwrap();
    let config = Config {
        api_key: Some("must-not-be-sent-to-discovery".into()),
        ..Config::default()
    };
    let startup = Startup::begin_in(
        directory.path(),
        &config,
        None,
        &AtomicBool::new(false),
        scope(directory.path()),
    )
    .unwrap();
    assert!(!startup.will_start_engine);
    let mut session = startup
        .prepare_with(
            0,
            &AtomicBool::new(false),
            &mut |_| {},
            fixture_launch("exit"),
        )
        .unwrap();
    assert_eq!(session.backend_opts.api_key.as_deref(), Some(LOCAL_KEY));
    session.cleanup().unwrap();
    engine.validate().unwrap();
    assert_eq!(std::fs::read(&runfile_path).unwrap(), bytes);
    drop(session);
    engine.child.cleanup().unwrap();
}

#[test]
fn borrowed_server_survives_session_exit() {
    let directory = tempfile::tempdir().unwrap();
    let mut engine = ready_fixture();
    let config = Config {
        api_base: Some(format!("http://127.0.0.1:{}/v1", engine.port)),
        api_key: Some("explicit-fixture-key".into()),
        ..Config::default()
    };
    let startup = Startup::begin_in(
        directory.path(),
        &config,
        None,
        &AtomicBool::new(false),
        scope(directory.path()),
    )
    .unwrap();
    let session = startup
        .prepare_with(
            0,
            &AtomicBool::new(false),
            &mut |_| {},
            fixture_launch("exit"),
        )
        .unwrap();
    assert_eq!(
        session.backend_opts.api_key.as_deref(),
        Some("explicit-fixture-key")
    );
    drop(session);
    engine.validate().unwrap();
    engine.child.cleanup().unwrap();
}

#[test]
fn startup_ambiguous_registration_is_nonmutating() {
    let directory = tempfile::tempdir().unwrap();
    let path = server::runfile_path(directory.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"invalid registration").unwrap();
    let config = Config::default();
    assert!(
        Startup::begin_in(
            directory.path(),
            &config,
            None,
            &AtomicBool::new(false),
            scope(directory.path())
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid registration");
    assert!(
        !directory
            .path()
            .join(".ferric/startup-preference.json")
            .exists()
    );
}

#[test]
fn stale_preferences_do_not_reuse_qualification() {
    let directory = tempfile::tempdir().unwrap();
    let path = model_fixture(directory.path());
    let startup = begin_local(directory.path());
    let Source::Local(local) = &startup.source else {
        unreachable!()
    };
    startup
        .state
        .write_preference(&local[0].preference())
        .unwrap();
    drop(startup);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(0);
    std::fs::write(&path, bytes).unwrap();
    let startup = begin_local(directory.path());
    assert_eq!(startup.preferred_index, None);
    assert!(
        startup.requires_model_choice,
        "one stale model must not be silently auto-selected"
    );
    assert!(startup.will_start_engine);
    assert_eq!(startup.config.ctx, None);
}

#[test]
fn startup_explain_is_read_only() {
    let directory = tempfile::tempdir().unwrap();
    model_fixture(directory.path());
    let description = describe(directory.path(), &Config::default(), None).unwrap();
    assert_eq!(description.context, 4096);
    assert_eq!(description.local_models.len(), 1);
    assert!(description.qualification.starts_with("unqualified"));
    assert!(!directory.path().join(storage::LOCK_FILE).exists());
    assert!(!directory.path().join(".ferric").exists());
    assert!(
        !serde_json::to_string(&description)
            .unwrap()
            .contains("api_key")
    );
}

#[test]
fn metadata_does_not_promote_authority() {
    let parsed = probe::parse_models(
        br#"{"data":[{"id":"model","context_length":999999,"qualified":true}]}"#,
    )
    .unwrap();
    assert_eq!(parsed, ["model"]);
    let directory = tempfile::tempdir().unwrap();
    model_fixture(directory.path());
    let startup = begin_local(directory.path());
    assert_eq!(startup.config.ctx, None);
    assert_eq!(startup.preferred_index, None);
}

#[test]
fn startup_probe_limits_matrix() {
    for body in [
        b"not JSON".as_slice(),
        br#"{"data":[]}"#,
        br#"{"data":[{"id":""}]}"#,
        br#"{"data":[{"id":"bad\u0000id"}]}"#,
    ] {
        assert!(probe::parse_models(body).is_err());
    }
    let excess = serde_json::json!({"data": (0..129).map(|index| serde_json::json!({"id":index.to_string()})).collect::<Vec<_>>()});
    assert!(probe::parse_models(&serde_json::to_vec(&excess).unwrap()).is_err());
    assert!(probe::parse_models(&vec![b' '; probe::BODY_LIMIT + 1]).is_err());
    for endpoint in [
        "file:///tmp/a",
        "http://user:secret@example.invalid/v1",
        "http://example.invalid/v1?key=secret",
        "http://example.invalid/v1#secret",
    ] {
        assert!(probe::endpoint(endpoint).is_err());
    }
    assert_eq!(
        probe::endpoint("http://example.invalid/v1/").unwrap(),
        "http://example.invalid/v1"
    );
}

#[test]
fn startup_bounded_version_probe() {
    let result = runtime::version(
        fixture_command("version", 0),
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    assert!(!result.is_empty());
    let expired = runtime::version(
        fixture_command("version", 0),
        &AtomicBool::new(false),
        Instant::now() - Duration::from_millis(1),
    )
    .unwrap_err();
    assert!(expired.to_string().contains("deadline"));
    let start = Instant::now();
    let timeout = runtime::version(
        fixture_command("absent", 0),
        &AtomicBool::new(false),
        start + Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(timeout.to_string().contains("deadline"));
    assert!(start.elapsed() < Duration::from_secs(2));
    let cancel = AtomicBool::new(true);
    assert!(
        runtime::version(
            fixture_command("absent", 0),
            &cancel,
            Instant::now() + Duration::from_secs(5)
        )
        .unwrap_err()
        .is_cancelled()
    );
}
