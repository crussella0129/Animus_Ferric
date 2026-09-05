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
    if let Some(path) = std::env::var_os("FERRIC_STARTUP_FIXTURE_PID_FILE") {
        std::fs::write(path, format!("{}\n", std::process::id())).unwrap();
    }
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
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                let request_deadline = (Instant::now() + Duration::from_secs(3)).min(deadline);
                let complete = loop {
                    if request.len() > 8192 || Instant::now() >= request_deadline {
                        eprintln!(
                            "startup fixture: request header limit/deadline, bytes={}",
                            request.len()
                        );
                        break false;
                    }
                    match stream.read(&mut buffer) {
                        Ok(0) => {
                            eprintln!("startup fixture: request closed before complete headers");
                            break false;
                        }
                        Ok(count) => {
                            if count > 8192_usize.saturating_sub(request.len()) {
                                eprintln!("startup fixture: request headers exceeded 8192 bytes");
                                break false;
                            }
                            request.extend_from_slice(&buffer[..count]);
                        }
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
                            eprintln!("startup fixture: request read failed: {:?}", error.kind());
                            break false;
                        }
                    }
                    if Instant::now() >= request_deadline {
                        eprintln!("startup fixture: complete-header deadline elapsed");
                        break false;
                    }
                    if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        break true;
                    }
                };
                if !complete {
                    continue;
                }
                let health = request.starts_with(b"GET /health ");
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

#[test]
fn startup_concurrent_invocations_serialize() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Condvar, Mutex};

    // A one-shot barrier with a deadline: a failed worker cannot leave the
    // other worker or the Cargo harness waiting forever at a rendezvous.
    fn rendezvous(barrier: &(Mutex<usize>, Condvar), participants: usize) -> Result<(), String> {
        let mut arrived = barrier.0.lock().map_err(|_| "barrier poisoned")?;
        *arrived += 1;
        barrier.1.notify_all();
        let (arrived, _) = barrier
            .1
            .wait_timeout_while(arrived, Duration::from_secs(5), |arrived| {
                *arrived < participants
            })
            .map_err(|_| "barrier poisoned")?;
        if *arrived == participants {
            Ok(())
        } else {
            Err("startup concurrency barrier exceeded five seconds".into())
        }
    }

    crate::test_process_containment::ensure_current_process_tree_is_contained().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    model_fixture(workspace.path());
    std::fs::create_dir(workspace.path().join(".ferric")).unwrap();
    let expert_config = workspace.path().join(".ferric/config.toml");
    let expert_bytes = b"temperature = 0.7\n";
    std::fs::write(&expert_config, expert_bytes).unwrap();
    let ready_to_start = (Mutex::new(0), Condvar::new());
    let attempts_finished = (Mutex::new(0), Condvar::new());
    let launches = AtomicUsize::new(0);
    let worker = || -> Result<bool, String> {
        rendezvous(&ready_to_start, 3)?;
        let attempt = Startup::begin_in(
            workspace.path(),
            &Config::default(),
            None,
            &AtomicBool::new(false),
            scope(workspace.path()),
        );
        // Retain an admitted Startup and its exact lock until both racing
        // attempts have resolved. The winner cannot release early and admit
        // a second sequential winner that merely looks concurrent.
        rendezvous(&attempts_finished, 2)?;
        let Ok(startup) = attempt else {
            return Ok(false);
        };
        let mut session = startup
            .prepare_with(
                0,
                &AtomicBool::new(false),
                &mut |_| {},
                |_, _, port, _, _| {
                    launches.fetch_add(1, Ordering::SeqCst);
                    Ok((
                        fixture_command("ready", port),
                        "concurrent source fixture".into(),
                    ))
                },
            )
            .map_err(|error| error.to_string())?;
        session.validate().map_err(|error| error.to_string())?;
        let retained = match &session.ownership {
            Ownership::Owned(engine) => LiveProcess::acquire(engine.child.tree.child().id())
                .map_err(|error| format!("retain winning engine: {error}"))?,
            _ => return Err("winning startup did not own its engine".into()),
        };
        session.cleanup().map_err(|error| error.to_string())?;
        if !retained
            .wait(Duration::from_secs(2))
            .map_err(|error| format!("observe winning engine exit: {error}"))?
        {
            return Err("winning startup engine remained alive after checked cleanup".into());
        }
        drop(session);
        Ok(true)
    };
    let results = std::thread::scope(|threads| {
        let first = threads.spawn(worker);
        let second = threads.spawn(worker);
        rendezvous(&ready_to_start, 3).unwrap();
        [
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
        ]
    });
    assert_eq!(results.into_iter().filter(|admitted| *admitted).count(), 1);
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    // Both attempts and all checked engine cleanup have finished; admission
    // can resume without deleting/recreating the persistent workspace lock.
    let next = begin_local(workspace.path());
    assert_eq!(next.preferred_index, Some(0));
    assert_eq!(std::fs::read(expert_config).unwrap(), expert_bytes);
}

#[test]
fn startup_prepare_cancellation_automatically_reaps_and_unlocks() {
    use std::sync::atomic::AtomicU16;
    struct CancelOnExit<'a>(&'a AtomicBool);
    impl Drop for CancelOnExit<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    for mode in ["absent", "loading"] {
        let workspace = tempfile::tempdir().unwrap();
        model_fixture(workspace.path());
        let pid_path = workspace.path().join("fixture-pid");
        let port = AtomicU16::new(0);
        let cancel = AtomicBool::new(false);
        let startup = begin_local(workspace.path());
        std::thread::scope(|threads| {
            let observer = threads.spawn(|| -> Result<(), String> {
                let _cancel_on_exit = CancelOnExit(&cancel);
                let outcome = (|| {
                    let deadline = Instant::now() + Duration::from_secs(4);
                    let pid = loop {
                        if let Ok(text) = std::fs::read_to_string(&pid_path)
                            && let Some(pid) = text
                                .strip_suffix('\n')
                                .and_then(|text| text.parse::<u32>().ok())
                        {
                            break pid;
                        }
                        if Instant::now() >= deadline {
                            return Err("fixture did not publish its PID".into());
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    };
                    // Observation never signals the process. Destructive
                    // authority remains solely in prepare_with's ProcessTree.
                    let process = LiveProcess::acquire(pid).map_err(|error| error.to_string())?;
                    loop {
                        let facts = process
                            .inspect(port.load(Ordering::Acquire))
                            .map_err(|error| error.to_string())?;
                        assert!(
                            facts
                                .identity
                                .argv
                                .iter()
                                .any(|argument| argument == "startup::tests::fixture_engine")
                        );
                        if mode == "absent" || facts.listener == ListenerState::OwnedByTarget {
                            break;
                        }
                        if Instant::now() >= deadline {
                            return Err("fixture did not acquire its listener".into());
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    cancel.store(true, Ordering::Release);
                    if process
                        .wait(Duration::from_secs(5))
                        .map_err(|error| error.to_string())?
                    {
                        Ok(())
                    } else {
                        Err("prepare cancellation left its exact process alive".into())
                    }
                })();
                cancel.store(true, Ordering::Release);
                outcome
            });
            let outcome =
                startup.prepare_with(0, &cancel, &mut |_| {}, |_, _, selected_port, _, _| {
                    port.store(selected_port, Ordering::Release);
                    let mut command = fixture_command(mode, selected_port);
                    command.env("FERRIC_STARTUP_FIXTURE_PID_FILE", &pid_path);
                    Ok((command, "cancellation source fixture".into()))
                });
            let error = match outcome {
                Err(error) => error,
                Ok(_) => panic!("cancelled preparation succeeded"),
            };
            observer.join().unwrap().unwrap();
            assert!(error.is_cancelled(), "{mode}: {error}");
        });
        let listener =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port.load(Ordering::Acquire)))
                .expect("cancelled preparation released its listener");
        drop(listener);
        assert!(
            !workspace
                .path()
                .join(".ferric/startup-preference.json")
                .exists()
        );
        let _next = begin_local(workspace.path());
    }
}

fn fixture_runfile(engine: &OwnedEngine, origin: &Path) -> server::ServerRunfile {
    server::ServerRunfile {
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
        origin_local_runfile: Some(origin.to_path_buf()),
    }
}

#[test]
fn startup_typed_refusal_matrix_preserves_resources() {
    use std::sync::atomic::AtomicUsize;
    for expected in ["stale", "conflict", "degraded", "unverifiable"] {
        let workspace = tempfile::tempdir().unwrap();
        model_fixture(workspace.path());
        let local = server::runfile_path(workspace.path());
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        let global_root = tempfile::tempdir().unwrap();
        let global = global_root.path().join("server.json");
        let mut discovery_scope = scope(workspace.path());
        let mut engine = if expected == "degraded" {
            let port = free_port();
            let engine = OwnedEngine::spawn(fixture_command("loading", port), port).unwrap();
            let deadline = Instant::now() + Duration::from_secs(4);
            while engine.listener().unwrap() != ListenerState::OwnedByTarget {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            engine
        } else {
            ready_fixture()
        };
        let record = fixture_runfile(&engine, &local);
        let local_bytes = if expected == "unverifiable" {
            b"invalid registration".to_vec()
        } else {
            serde_json::to_vec(&record).unwrap()
        };
        std::fs::write(&local, &local_bytes).unwrap();
        let global_bytes = if expected == "conflict" {
            let mut conflicting = record.clone();
            conflicting.context_size = Some(8192);
            let bytes = serde_json::to_vec(&conflicting).unwrap();
            std::fs::write(&global, &bytes).unwrap();
            discovery_scope.global = Some(global.clone());
            Some(bytes)
        } else {
            None
        };
        if expected == "stale" {
            engine.child.cleanup().unwrap();
        }
        let discovery = server::discover_managed_server_in(&discovery_scope);
        assert!(
            matches!(
                (&discovery.state, expected),
                (ManagedServerState::StaleOnly { .. }, "stale")
                    | (ManagedServerState::Conflict { .. }, "conflict")
                    | (ManagedServerState::Degraded { .. }, "degraded")
                    | (ManagedServerState::Unverifiable { .. }, "unverifiable")
            ),
            "expected {expected}, observed {:?}",
            discovery.state
        );
        let launches = AtomicUsize::new(0);
        let result = Startup::begin_in(
            workspace.path(),
            &Config::default(),
            None,
            &AtomicBool::new(false),
            discovery_scope,
        )
        .and_then(|startup| {
            startup.prepare_with(
                0,
                &AtomicBool::new(false),
                &mut |_| {},
                |_, _, port, _, _| {
                    launches.fetch_add(1, Ordering::SeqCst);
                    Ok((
                        fixture_command("ready", port),
                        "unexpected fixture launch".into(),
                    ))
                },
            )
        });
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("unsafe {expected} state was admitted"),
        };
        assert!(error.to_string().contains("ferric server status"));
        assert_eq!(launches.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read(&local).unwrap(), local_bytes);
        if let Some(bytes) = global_bytes {
            assert_eq!(std::fs::read(&global).unwrap(), bytes);
        }
        assert!(
            !workspace
                .path()
                .join(".ferric/startup-preference.json")
                .exists()
        );
        if expected != "stale" {
            engine
                .validate()
                .expect("refusal did not signal the existing process/listener");
        }
        engine.child.cleanup().unwrap();
        storage::WorkspaceState::acquire(workspace.path()).unwrap();
    }
}

#[test]
fn startup_expired_180_second_budget_precedes_ready_effects() {
    assert_eq!(STARTUP_LIMIT, Duration::from_secs(180));
    let mut engine = ready_fixture();
    let trap = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    trap.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/v1", trap.local_addr().unwrap());
    let error = wait_ready(
        &mut engine,
        &endpoint,
        &AtomicBool::new(false),
        Instant::now() - Duration::from_millis(1),
    )
    .unwrap_err();
    assert!(error.to_string().contains("exceeded 180 seconds"));
    assert_eq!(
        trap.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "expired startup must not probe even a ready engine"
    );
    engine.child.cleanup().unwrap();
}

#[test]
fn startup_fixture_keeps_fragmented_headers_bounded() {
    let mut engine = ready_fixture();
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, engine.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stream.write_all(b"GET /hea").unwrap();
    std::thread::sleep(Duration::from_millis(350));
    stream
        .write_all(b"lth HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(
        response.ends_with("\r\n\r\n{}"),
        "fragmented health request was not preserved"
    );
    drop(stream);
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, engine.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream.write_all(b"GET /unfinished").unwrap();
    let started = Instant::now();
    let mut byte = [0_u8; 1];
    assert_eq!(
        stream.read(&mut byte).unwrap(),
        0,
        "stalled fixture request must close at its absolute deadline"
    );
    assert!(started.elapsed() < Duration::from_secs(4));
    engine.child.cleanup().unwrap();
}
