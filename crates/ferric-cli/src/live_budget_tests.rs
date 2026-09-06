//! Source-owned qualification of explicit budgets, not an application or
//! hardware calibration. The real nested-engine outer owner is qualified on
//! Windows Jobs only. Unix process groups do not contain a separately grouped
//! PreparedSession engine after forced owner death; do not claim that parity.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ferric_core::{ActionProtocol, HarnessPolicy, ModelProfile, policy_for, resolve_output_budget};
use ferric_guard::{Provenance, SinkPolicy, Workspace};
use ferric_provider::{
    Capabilities, Completion, CompletionRequest, OpenAiConfig, OpenAiProvider, Provider,
    ProviderError, SamplingParams,
};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const LIVE_EXECUTION: Duration = Duration::from_secs(150);
const LIVE_ACCEPTANCE: Duration = Duration::from_secs(180);
const LIVE_SETUP: Duration = Duration::from_secs(90);
const LIVE_REQUEST: Duration = Duration::from_secs(30);
const CAP: u32 = 1024;
const CONTEXT: u32 = 4096;
const HASH_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum HashFailure {
    Cancelled,
    Deadline,
    Io(String),
}

impl std::fmt::Display for HashFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("identity hashing cancelled"),
            Self::Deadline => formatter.write_str("identity hashing reached the setup deadline"),
            Self::Io(error) => write!(formatter, "identity hashing I/O: {error}"),
        }
    }
}

fn hash_admission(cancel: &AtomicBool, deadline: Instant) -> Result<(), HashFailure> {
    if cancel.load(Ordering::Acquire) {
        Err(HashFailure::Cancelled)
    } else if Instant::now() >= deadline {
        Err(HashFailure::Deadline)
    } else {
        Ok(())
    }
}

/// Fixture-only bulk identity work shares the original setup deadline. Each
/// bounded read/update checks cancellation; a synchronous OS read can still
/// stall, so the independently supervised parent remains the outer backstop.
fn identity_hash(
    path: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<String, HashFailure> {
    hash_admission(cancel, deadline)?;
    let mut file = std::fs::File::open(path).map_err(|error| HashFailure::Io(error.to_string()))?;
    hash_reader(&mut file, cancel, deadline, |_| {})
}

fn hash_reader(
    reader: &mut impl Read,
    cancel: &AtomicBool,
    deadline: Instant,
    mut after_chunk: impl FnMut(usize),
) -> Result<String, HashFailure> {
    hash_admission(cancel, deadline)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; HASH_CHUNK_BYTES];
    loop {
        hash_admission(cancel, deadline)?;
        let read = reader
            .read(&mut buffer)
            .map_err(|error| HashFailure::Io(error.to_string()))?;
        hash_admission(cancel, deadline)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        after_chunk(read);
        hash_admission(cancel, deadline)?;
    }
    let digest = hex::encode(digest.finalize());
    hash_admission(cancel, deadline)?;
    Ok(digest)
}

/// The on-disk journal survives an outer timeout; successful/failing child
/// reports also embed it. Times are observations, never guessed allocations.
struct StageJournal {
    file: std::fs::File,
    started: Instant,
    records: Vec<Value>,
}

impl StageJournal {
    fn new(root: &Path, started: Instant) -> Self {
        Self {
            file: OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("stages.jsonl"))
                .unwrap(),
            started,
            records: Vec::new(),
        }
    }

    fn record(&mut self, stage: &str, boundary: &str) {
        let record = json!({"stage":stage,"boundary":boundary,"elapsed_ms":self.started.elapsed().as_millis()});
        serde_json::to_writer(&mut self.file, &record).unwrap();
        self.file.write_all(b"\n").unwrap();
        self.file.flush().unwrap();
        self.records.push(record);
    }
}

#[test]
fn live_identity_hash_known_vector_and_chunks() {
    let cancel = AtomicBool::new(false);
    for (bytes, expected) in [
        (
            b"".as_slice(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".as_slice(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
    ] {
        assert_eq!(
            hash_reader(
                &mut std::io::Cursor::new(bytes),
                &cancel,
                Instant::now() + Duration::from_secs(30),
                |_| {}
            )
            .unwrap(),
            expected
        );
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source-generated-identity.bin");
    let bytes = vec![0xa5; 16 * 1024 * 1024];
    let expected = ferric_bench::sha256_bytes(&bytes);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    drop(file);
    let started = Instant::now();
    let actual = identity_hash(&path, &cancel, started + Duration::from_secs(30)).unwrap();
    let elapsed = started.elapsed();
    assert_eq!(actual, expected);
    println!(
        "IDENTITY_HASH_EVIDENCE={}",
        json!({
            "bytes":bytes.len(),"chunk_bytes":HASH_CHUNK_BYTES,"elapsed_ms":elapsed.as_millis(),
            "sha256":actual,"debug_assertions":cfg!(debug_assertions),
            "target_os":std::env::consts::OS,"target_arch":std::env::consts::ARCH,
            "qualification":"fixture-local SHA-256 only; no inference or machine-speed acceptance threshold",
        })
    );
    root.close().unwrap();
}

#[test]
fn live_identity_hash_refuses_cancel_and_deadline_before_read() {
    struct NoRead;
    impl Read for NoRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("inadmissible hashing must not read");
        }
    }
    let cancel = AtomicBool::new(true);
    assert_eq!(
        hash_reader(
            &mut NoRead,
            &cancel,
            Instant::now() + Duration::from_secs(30),
            |_| {}
        ),
        Err(HashFailure::Cancelled)
    );
    cancel.store(false, Ordering::Release);
    assert_eq!(
        hash_reader(&mut NoRead, &cancel, Instant::now(), |_| {}),
        Err(HashFailure::Deadline)
    );
}

#[test]
fn live_identity_hash_cancels_between_chunks_without_partial_digest() {
    let cancel = AtomicBool::new(false);
    let mut reader = std::io::Cursor::new(vec![0x5a; 3 * HASH_CHUNK_BYTES]);
    let mut chunks = 0;
    let result = hash_reader(
        &mut reader,
        &cancel,
        Instant::now() + Duration::from_secs(30),
        |read| {
            assert_eq!(read, HASH_CHUNK_BYTES);
            chunks += 1;
            cancel.store(true, Ordering::Release);
        },
    );
    assert_eq!(result, Err(HashFailure::Cancelled));
    assert_eq!(chunks, 1);
    assert_eq!(reader.position(), HASH_CHUNK_BYTES as u64);
}

#[test]
fn live_identity_hash_checks_cancel_after_read_before_update() {
    struct CancellingReader<'a>(&'a AtomicBool);
    impl Read for CancellingReader<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            bytes[0] = 42;
            self.0.store(true, Ordering::Release);
            Ok(1)
        }
    }
    let cancel = AtomicBool::new(false);
    assert_eq!(
        hash_reader(
            &mut CancellingReader(&cancel),
            &cancel,
            Instant::now() + Duration::from_secs(30),
            |_| panic!("cancelled read must not reach hashing")
        ),
        Err(HashFailure::Cancelled)
    );
}

#[test]
fn live_stage_journal_retains_timing_and_partial_outer_timeout_bytes() {
    let root = tempfile::tempdir().unwrap();
    let mut stages = StageJournal::new(root.path(), Instant::now());
    stages.record("model_hash", "start");
    stages.record("model_hash", "failed");
    let expected = stages.records.clone();
    drop(stages);
    let path = root.path().join("stages.jsonl");
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"stage\":").unwrap();
    drop(file);
    let original = std::fs::read(&path).unwrap();
    let journal = read_stage_journal(root.path()).unwrap().unwrap();
    assert_eq!(
        journal["raw_utf8"],
        String::from_utf8(original.clone()).unwrap()
    );
    assert_eq!(journal["sha256"], ferric_bench::sha256_bytes(&original));
    assert_eq!(journal["final_newline"], false);
    assert_eq!(journal["records"][0]["record"], expected[0]);
    assert_eq!(journal["records"][1]["record"], expected[1]);
    assert!(
        expected[1]["elapsed_ms"].as_u64().unwrap() >= expected[0]["elapsed_ms"].as_u64().unwrap()
    );
    assert!(journal["records"][2]["parse_error"].is_string());
    assert_eq!(
        journal["records"][2]["raw_hex"],
        hex::encode(b"{\"stage\":")
    );
    root.close().unwrap();
}

fn write_new_json(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

/// A stoppable watchdog, not a detached sleeper. Cleanup always joins it;
/// successful early completion never waits for the original phase deadline.
struct Watchdog {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<Result<(), String>>>,
    fired: Arc<AtomicBool>,
}

impl Watchdog {
    fn new(cancel: Arc<AtomicBool>, limit: Duration, root: &Path, phase: &str) -> Self {
        let (stop, receiver) = mpsc::channel();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_worker = fired.clone();
        let marker = root.join(format!("{phase}-watchdog-fired.json"));
        let thread = std::thread::spawn(move || {
            if matches!(
                receiver.recv_timeout(limit),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                cancel.store(true, Ordering::Release);
                fired_worker.store(true, Ordering::Release);
                write_new_json(
                    &marker,
                    &json!({"cancel_requested":true,"deadline_ms":limit.as_millis()}),
                )?;
            }
            Ok(())
        });
        Self {
            stop,
            thread: Some(thread),
            fired,
        }
    }

    fn finish(&mut self) -> Result<bool, String> {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !thread.is_finished() {
                if Instant::now() >= deadline {
                    ferric_process::abort_on_cleanup_failure(
                        "fixture watchdog failed to join",
                        "two-second join bound",
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            thread
                .join()
                .map_err(|_| "fixture watchdog panicked".to_string())??;
        }
        Ok(self.fired.load(Ordering::Acquire))
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            // finish() returned only after join; an I/O error or joined panic
            // is an ordinary fixture failure, not unproved worker cleanup.
            if std::thread::panicking() {
                eprintln!("joined fixture watchdog failed during unwind: {error}");
            } else {
                panic!("joined fixture watchdog failed: {error}");
            }
        }
    }
}

struct RecordingProvider {
    inner: OpenAiProvider,
    exchanges: Mutex<Vec<Value>>,
    root: PathBuf,
}

#[async_trait]
impl Provider for RecordingProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        let request_record = json!({
            "observation_layer":"provider_admission", "messages":request.messages,
            "sampling":request.sampling, "tools":request.tools, "constraint":request.constraint,
        });
        let first = {
            let mut exchanges = self.exchanges.lock().unwrap();
            let first = exchanges.is_empty();
            exchanges.push(json!({"request":request_record,"response":null}));
            first
        };
        if first {
            write_new_json(
                &self.root.join("request-entered.json"),
                &json!({"max_tokens":request.sampling.max_tokens}),
            )
            .map_err(ProviderError::Backend)?;
        }
        let result = self.inner.complete(request, cancel).await;
        let response = match &result {
            Ok(completion) => {
                json!({"message":completion.message,"input_tokens":completion.input_tokens,
                "output_tokens":completion.output_tokens,"truncated":completion.truncated})
            }
            Err(error) => json!({"error":error.to_string(),
                "provider_cancel_observed":matches!(error, ProviderError::Backend(message) if message == "Interrupted")}),
        };
        self.exchanges.lock().unwrap().last_mut().unwrap()["response"] = response;
        result
    }
}

fn parent_run(mode: &str, root: &Path, execution: Duration) -> Result<Value, String> {
    let started = Instant::now();
    crate::test_process_containment::ensure_current_process_tree_is_contained()?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .args([
            "--ignored",
            "--exact",
            "live_budget_tests::live_budget_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FERRIC_BUDGET_CHILD_MODE", mode)
        .env("FERRIC_BUDGET_CHILD_ROOT", root);
    let outcome = ferric_process::run_bounded(
        &mut command,
        execution,
        ferric_process::CapturePlan::head(1024 * 1024, 128 * 1024),
    )
    .map_err(|error| error.to_string())?;
    let total = started.elapsed();
    let child = root.join("child-report.json");
    let child = if child.is_file() {
        Some(
            serde_json::from_slice::<Value>(
                &std::fs::read(&child).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let markers: Vec<Value> = ["setup-entered", "setup-watchdog-fired", "request-entered", "request-watchdog-fired", "engine-listener", "engine-completion"]
        .into_iter().filter_map(|name| {
            let path = root.join(format!("{name}.json"));
            path.is_file().then(|| json!({"name":name,"data":serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap()}))
        }).collect();
    let stage_journal = read_stage_journal(root)?;
    Ok(json!({
        "schema_version":1,"mode":mode,"parent_execution_budget_ms":execution.as_millis(),
        "fixture_compilation":{"debug_assertions":cfg!(debug_assertions),"target_os":std::env::consts::OS,"target_arch":std::env::consts::ARCH},
        "parent_cleanup_budget_ms":ferric_process::CLEANUP_TIMEOUT.as_millis(),
        "parent_execution_timeout":outcome.timed_out,"parent_exit_code":outcome.exit_code,
        "parent_spawn_ms":outcome.spawn_wall.as_millis(),"parent_execution_ms":outcome.wall.as_millis(),
        "parent_total_ms":total.as_millis(),"parent_checked_process_scope_cleanup":true,
        "nested_engine_outer_containment":if cfg!(windows) {"windows_jobs"} else {"not_qualified; cooperative phase cleanup only"},
        "child":child,"phase_markers":markers,"stage_journal":stage_journal,
        "stdout":String::from_utf8_lossy(&outcome.stdout),"stderr":String::from_utf8_lossy(&outcome.stderr),
        "qualification":"explicit-budget prepared-path smoke only; no application, hardware calibration or skill-support verdict",
    }))
}

fn read_stage_journal(root: &Path) -> Result<Option<Value>, String> {
    let bytes = match std::fs::read(root.join("stages.jsonl")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read stage journal: {error}")),
    };
    // An outer kill may interrupt the last write. Retain the exact bytes even
    // when they are incomplete; parsing never turns them into a completed stage.
    let text = String::from_utf8(bytes.clone());
    let records: Vec<Value> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| match serde_json::from_slice::<Value>(line) {
            Ok(record) => json!({"record":record}),
            Err(error) => json!({"parse_error":error.to_string(),"raw_hex":hex::encode(line)}),
        })
        .collect();
    Ok(Some(json!({
        "raw_utf8":text.as_ref().ok(),
        "raw_hex":text.is_err().then(|| hex::encode(&bytes)),
        "sha256":ferric_bench::sha256_bytes(&bytes),
        "final_newline":bytes.ends_with(b"\n"),"records":records,
    })))
}

#[test]
#[ignore = "opt-in existing GGUF/runtime on qualified Windows host; FERRIC_LIVE_MODEL required"]
fn real_model_explicit_budget_smoke() {
    let acceptance_started = Instant::now();
    if !cfg!(windows) {
        panic!(
            "NON-PASS: nested-engine forced outer cleanup is Windows-qualified only; Unix requires a separately qualified namespace supervisor"
        );
    }
    let model = std::env::var_os("FERRIC_LIVE_MODEL").expect(
        "NON-PASS: FERRIC_LIVE_MODEL must select the existing 7B GGUF; no download or silent skip",
    );
    assert!(
        Path::new(&model).is_file(),
        "NON-PASS: selected existing model is missing"
    );
    let root = tempfile::tempdir().unwrap();
    let report =
        parent_run("live", root.path(), LIVE_EXECUTION).expect("parent supervision failed");
    if let Some(directory) = std::env::var_os("FERRIC_BUDGET_LIVE_EVIDENCE_DIR") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        write_new_json(&directory.join("live-budget-report.json"), &report).unwrap();
    }
    println!("LIVE_BUDGET_EVIDENCE={report}");
    assert_eq!(report["parent_execution_timeout"], false, "{report}");
    assert_eq!(report["parent_exit_code"], 0, "{report}");
    assert!(
        report["parent_total_ms"].as_u64().unwrap() < LIVE_ACCEPTANCE.as_millis() as u64,
        "{report}"
    );
    assert_eq!(report["child"]["passed"], true, "{report}");
    assert_eq!(
        report["child"]["owned_engine_checked_cleanup"], true,
        "{report}"
    );
    assert_eq!(report["child"]["request_cap_verified"], true, "{report}");
    root.close()
        .expect("ordinary fixture workspace teardown failed");
    assert!(
        acceptance_started.elapsed() < LIVE_ACCEPTANCE,
        "fixture acceptance including evidence capture exceeded its 180-second ceiling"
    );
}

#[test]
fn live_budget_fixture_stalled_phases_reap() {
    for mode in [
        "setup-cancel",
        "request-cancel",
        "setup-sync-stall",
        "request-sync-stall",
    ] {
        let root = tempfile::tempdir().unwrap();
        let outer = if mode.ends_with("sync-stall") {
            Duration::from_secs(4)
        } else {
            Duration::from_secs(25)
        };
        let report = parent_run(mode, root.path(), outer).unwrap();
        assert_eq!(
            report["parent_checked_process_scope_cleanup"], true,
            "{report}"
        );
        if mode.ends_with("sync-stall") {
            assert_eq!(report["parent_execution_timeout"], true, "{report}");
            let phase = if mode.starts_with("setup") {
                "setup"
            } else {
                "request"
            };
            assert!(
                root.path()
                    .join(format!("{phase}-watchdog-fired.json"))
                    .is_file(),
                "watchdog must actually cancel before the independent outer deadline: {report}"
            );
            assert!(
                !root.path().join("engine-listener.json").is_file(),
                "portable synchronous modes must never launch a separately grouped engine"
            );
        } else {
            assert_eq!(report["parent_execution_timeout"], false, "{report}");
            assert_eq!(report["parent_exit_code"], 0, "{report}");
            assert_eq!(report["child"]["phase_cancelled"], true, "{report}");
            assert_eq!(
                report["child"]["owned_engine_checked_cleanup"], true,
                "{report}"
            );
            assert_eq!(report["child"]["watchdogs_joined"], true, "{report}");
            assert!(
                root.path().join("engine-listener.json").is_file(),
                "actual prepared engine must have started"
            );
            if mode == "request-cancel" {
                assert!(root.path().join("engine-completion.json").is_file());
                assert_eq!(
                    report["child"]["request"]["watchdog_fired"], true,
                    "{report}"
                );
                assert_eq!(
                    report["child"]["request"]["provider_cancel_observed"], true,
                    "{report}"
                );
            }
        }
        if let Ok(bytes) = std::fs::read(root.path().join("engine-listener.json")) {
            let listener: Value = serde_json::from_slice(&bytes).unwrap();
            let _closed = TcpListener::bind((
                Ipv4Addr::LOCALHOST,
                listener["port"].as_u64().unwrap() as u16,
            ))
            .expect("source-owned engine listener must be released after checked cleanup");
        }
        println!("STALLED_PHASE_EVIDENCE={report}");
    }
}

fn fixture_model(root: &Path) -> PathBuf {
    let mut header = [0u8; 24];
    header[..4].copy_from_slice(b"GGUF");
    header[4..8].copy_from_slice(&3u32.to_le_bytes());
    let model = root.join("fixture.gguf");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&model)
        .unwrap();
    file.write_all(&header).unwrap();
    model
}

fn source_engine(port: u16, mode: &str, root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "live_budget_tests::live_budget_engine",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FERRIC_BUDGET_ENGINE_PORT", port.to_string())
        .env("FERRIC_BUDGET_ENGINE_MODE", mode)
        .env("FERRIC_BUDGET_CHILD_ROOT", root);
    command
}

#[test]
#[ignore = "finite source body invoked only by Cargo parent budget fixtures"]
fn live_budget_child() {
    let Ok(mode) = std::env::var("FERRIC_BUDGET_CHILD_MODE") else {
        return;
    };
    crate::test_process_containment::ensure_current_process_tree_is_contained().unwrap();
    let root = PathBuf::from(std::env::var_os("FERRIC_BUDGET_CHILD_ROOT").unwrap());
    let report = child_body(&mode, &root);
    write_new_json(&root.join("child-report.json"), &report).unwrap();
    assert_eq!(report["passed"], true, "{report}");
}

fn child_body(mode: &str, root: &Path) -> Value {
    let live = mode == "live";
    if live && !cfg!(windows) {
        return json!({"passed":false,"error":"NON-PASS: nested live engine outer ownership not qualified on Unix"});
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let setup_limit = if live {
        LIVE_SETUP
    } else if mode == "request-cancel" {
        Duration::from_secs(3)
    } else {
        Duration::from_secs(1)
    };
    let setup_started = Instant::now();
    let setup_deadline = setup_started + setup_limit;
    let mut setup_watchdog = Watchdog::new(cancel.clone(), setup_limit, root, "setup");
    let mut stages = StageJournal::new(root, setup_started);
    stages.record("setup", "start");
    write_new_json(
        &root.join("setup-entered.json"),
        &json!({"mode":mode,"limit_ms":setup_limit.as_millis()}),
    )
    .unwrap();
    if mode == "setup-sync-stall" {
        // No engine or native child exists in this deliberately unresponsive
        // setup phase. The parent owns and reaps the only source child.
        stages.record("setup_sync_stall_no_engine", "start");
        std::thread::sleep(Duration::from_secs(20));
    }
    if mode == "request-sync-stall" {
        setup_watchdog.finish().unwrap();
        stages.record("setup", "end");
        return pure_request_stall(root, &mut stages);
    }
    stages.record("model_selection", "start");
    let model = if live {
        PathBuf::from(std::env::var_os("FERRIC_LIVE_MODEL").expect("existing model required"))
    } else {
        fixture_model(root)
    };
    stages.record("model_selection", "end");
    let cfg = crate::config::Config {
        ctx: Some(CONTEXT),
        ..Default::default()
    };
    stages.record("startup_begin", "start");
    let start = crate::startup::test_support::begin(root, &cfg, Some(&model), &cancel);
    stages.record(
        "startup_begin",
        if start.is_ok() { "end" } else { "failed" },
    );
    let preparation = start.and_then(|start| {
        assert!(
            start.will_start_engine,
            "test must own its engine, never borrow a user's server"
        );
        stages.record("engine_prepare", "start");
        let prepared = if live {
            start.prepare(0, cancel.clone(), &mut |_| {})
        } else {
            crate::startup::test_support::prepare(start, 0, cancel.clone(), &mut |_| {}, |port| {
                source_engine(port, mode, root)
            })
        };
        // prepare's cancellation/error path owns its checked engine teardown;
        // it is included in this duration, not guessed as a separate interval.
        stages.record(
            "engine_prepare",
            if prepared.is_ok() { "end" } else { "failed" },
        );
        prepared
    });
    let mut prepared = match preparation {
        Ok(prepared) => prepared,
        Err(error) => {
            let fired = setup_watchdog.finish().unwrap();
            stages.record("setup", "failed");
            return json!({"passed":mode=="setup-cancel" && error.is_cancelled() && fired,
                "phase":"setup","phase_cancelled":error.is_cancelled(),"error":error.to_string(),
                "setup_watchdog_fired":fired,"watchdogs_joined":true,
                "owned_engine_checked_cleanup":error.is_cancelled(),"setup_ms":setup_started.elapsed().as_millis(),
                "stage_observations":stages.records});
        }
    };
    let identity = if live {
        let runtime_path = prepared
            .engine_identity
            .split_once(" — ")
            .map(|(path, _)| PathBuf::from(path));
        stages.record("model_hash", "start");
        let model_hash_started = Instant::now();
        let model_digest = identity_hash(&model, &cancel, setup_deadline);
        let model_hash_ms = model_hash_started.elapsed().as_millis();
        stages.record(
            "model_hash",
            if model_digest.is_ok() {
                "end"
            } else {
                "failed"
            },
        );
        let (runtime_digest, runtime_hash_ms) = if model_digest.is_ok() {
            if let Some(path) = runtime_path.as_ref() {
                stages.record("runtime_hash", "start");
                let runtime_hash_started = Instant::now();
                let digest = identity_hash(path, &cancel, setup_deadline);
                let elapsed = runtime_hash_started.elapsed().as_millis();
                stages.record(
                    "runtime_hash",
                    if digest.is_ok() { "end" } else { "failed" },
                );
                (Some(digest), Some(elapsed))
            } else {
                stages.record("runtime_hash", "skipped_missing_path");
                (None, None)
            }
        } else {
            stages.record("runtime_hash", "skipped_model_hash_failed");
            (None, None)
        };
        json!({"model_file":model,"model_sha256":model_digest.as_ref().ok(),"model_bytes":std::fs::metadata(&model).ok().map(|m|m.len()),
            "runtime":prepared.engine_identity,"runtime_sha256":runtime_digest.as_ref().and_then(|result|result.as_ref().ok()),
            "model_hash_error":model_digest.as_ref().err().map(|error|error.to_string()),
            "runtime_hash_error":runtime_digest.as_ref().and_then(|result|result.as_ref().err()).map(|error|error.to_string()),
            "model_hash_ms":model_hash_ms,"runtime_hash_ms":runtime_hash_ms,
            "runtime_hash_skipped":runtime_digest.is_none(),
            "verified_endpoint":prepared.backend_opts.api_base,"verified_model":prepared.model,
            "context":prepared.context,"params_b_declared":7.0,"cpu_only":true,"gpu_layers":0,"parallel":1,
            "identity_hashes_available":model_digest.is_ok() && runtime_digest.is_some_and(|result|result.is_ok())})
    } else {
        json!({"model_file":"source-defined fixture","runtime":prepared.engine_identity,"verified_endpoint":prepared.backend_opts.api_base,"verified_model":prepared.model,"context":prepared.context})
    };
    let setup_fired = setup_watchdog.finish().unwrap();
    let setup_expired = cancel.load(Ordering::Acquire) || setup_started.elapsed() >= setup_limit;
    let identity_unavailable = live && identity["identity_hashes_available"] != true;
    if setup_expired || identity_unavailable {
        stages.record("setup", "failed");
        let setup_ms = setup_started.elapsed().as_millis();
        stages.record("owned_cleanup", "start");
        let cleanup = prepared.cleanup();
        stages.record(
            "owned_cleanup",
            if cleanup.is_ok() { "end" } else { "failed" },
        );
        return json!({"passed":false,"phase":"setup","phase_cancelled":setup_expired,"setup_watchdog_fired":setup_fired,
            "error":if setup_expired {"fixture setup deadline"} else {"identity hashing unavailable"},
            "watchdogs_joined":true,"owned_engine_checked_cleanup":cleanup.is_ok(),"identity":identity,
            "cleanup_error":cleanup.err().map(|error|error.to_string()),"setup_ms":setup_ms,"stage_observations":stages.records});
    }
    stages.record("setup", "end");
    let setup_ms = setup_started.elapsed().as_millis();
    let request_limit = if live {
        LIVE_REQUEST
    } else {
        Duration::from_secs(3)
    };
    let watchdog_limit = if live {
        LIVE_REQUEST
    } else {
        Duration::from_secs(1)
    };
    stages.record("request", "start");
    let result = request_phase(
        root,
        &prepared,
        cancel.clone(),
        request_limit,
        watchdog_limit,
    );
    stages.record("request", "end");
    stages.record("owned_cleanup", "start");
    let cleanup = prepared.cleanup();
    stages.record(
        "owned_cleanup",
        if cleanup.is_ok() { "end" } else { "failed" },
    );
    let mut report = json!({"identity":identity,"setup_ms":setup_ms,"setup_watchdog_fired":setup_fired,
        "request":result,"watchdogs_joined":true,"owned_engine_checked_cleanup":cleanup.is_ok(),
        "cleanup_error":cleanup.err().map(|error|error.to_string()),"stage_observations":stages.records});
    let phase_cancelled = report["request"]["cancel_requested"].as_bool() == Some(true);
    report["phase_cancelled"] = json!(phase_cancelled);
    let passed = if live {
        report["request"]["stop"] == "task_complete"
            && !phase_cancelled
            && report["request"]["within_request_deadline"] == true
            && report["identity"]["identity_hashes_available"] == true
    } else {
        mode == "request-cancel"
            && phase_cancelled
            && report["request"]["watchdog_fired"] == true
            && report["request"]["provider_cancel_observed"] == true
            && report["request"]["within_request_deadline"] == true
    };
    report["request_cap_verified"] = report["request"]["request_cap_verified"].clone();
    report["passed"] = json!(
        passed
            && report["owned_engine_checked_cleanup"] == true
            && report["request_cap_verified"] == true
    );
    report
}

fn request_phase(
    root: &Path,
    prepared: &crate::startup::PreparedSession,
    cancel: Arc<AtomicBool>,
    limit: Duration,
    watchdog_limit: Duration,
) -> Value {
    prepared.validate().unwrap();
    assert_eq!(prepared.context, CONTEXT);
    assert_eq!(
        prepared.ownership_label(),
        "owned foreground (closed on exit)"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let provider = runtime.block_on(async {
        OpenAiProvider::for_prepared_endpoint(OpenAiConfig {
            base_url: prepared.backend_opts.api_base.clone().unwrap(),
            api_key: prepared.backend_opts.api_key.clone().unwrap(),
            model: prepared.model.clone(),
        })
        .unwrap()
    });
    let provider = RecordingProvider {
        inner: provider,
        exchanges: Mutex::new(Vec::new()),
        root: root.into(),
    };
    let (trace_path, file) = prepared.create_trace_file("q-live-budget.jsonl").unwrap();
    let mut trace = JsonlSink::from_file(file, "live-budget").unwrap();
    let mut policy = policy_for(&ModelProfile {
        params_b: 7.0,
        quant: "Q4_K_M".into(),
        ctx: CONTEXT,
        family: "qwen2.5-coder".into(),
        measured_level: None,
    });
    let budget = resolve_output_budget(&policy, CONTEXT, Some(CAP)).unwrap();
    policy.max_output_tokens = budget.effective;
    policy.output_budget = Some(budget.clone());
    let workspace = Workspace::new(root).unwrap();
    let registry = ferric_tools::Registry::new();
    let mut watchdog = Watchdog::new(cancel.clone(), watchdog_limit, root, "request");
    let started = Instant::now();
    let outcome = runtime.block_on(async {
        tokio::time::timeout(limit, ferric_loop::run(ferric_loop::RunArgs {
            provider:&provider,registry:&registry,workspace:&workspace,policy:&policy,
            protocol:ActionProtocol::ConstrainedJson,harness_policy:Some(HarnessPolicy::Legacy),
            sampling:SamplingParams {temperature:0.0,max_tokens:CAP,..Default::default()},
            sleeper:&ferric_loop::ThreadSleeper,system_prompt:Some("Return the task_complete action immediately with summary 'Ferric budget smoke complete'. This is a tiny connectivity smoke; do not request input or perform any other action."),
            prompt_lineage:None,media:Vec::new(),stream_sink:None,resume:None,answer:None,
            cancel_flag:Some(cancel.clone()),provenance:Provenance::Clean,sink_policy:SinkPolicy::deny(),hooks:None,edit_approver:None,
        }, &mut trace, Some("Complete this smoke now using task_complete."))).await
    });
    let request_duration = started.elapsed();
    let within_request_deadline = request_duration < limit;
    if outcome.is_err() || !within_request_deadline {
        cancel.store(true, Ordering::Release);
    }
    let watchdog_fired = watchdog.finish().unwrap();
    drop(trace);
    let records = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let observed: Vec<_> = records
        .iter()
        .filter_map(|record| {
            if let ParsedEvent::Known(Event::MainActionBudget { budget, .. }) = &record.event {
                Some(budget)
            } else {
                None
            }
        })
        .collect();
    let exchanges = provider.exchanges.into_inner().unwrap();
    let provider_cancel_observed = exchanges
        .iter()
        .any(|exchange| exchange["response"]["provider_cancel_observed"] == true);
    let verified = !observed.is_empty()
        && observed.iter().all(|actual| **actual == budget)
        && !exchanges.is_empty()
        && exchanges
            .iter()
            .all(|exchange| exchange["request"]["sampling"]["max_tokens"] == CAP);
    let (stop, error) = match outcome {
        Ok(Ok(outcome)) => (Some(outcome.stop.as_str()), None),
        Ok(Err(error)) => (None, Some(error.to_string())),
        Err(error) => (None, Some(format!("fixture request deadline: {error}"))),
    };
    json!({"stop":stop,"error":error,"duration_ms":request_duration.as_millis(),"within_request_deadline":within_request_deadline,"request_deadline_ms":limit.as_millis(),
        "watchdog_deadline_ms":watchdog_limit.as_millis(),"provider_cancel_observed":provider_cancel_observed,
        "cancel_requested":cancel.load(Ordering::Acquire),"watchdog_fired":watchdog_fired,"watchdog_joined":true,
        "request_cap_verified":verified,"output_budget":budget,"exchanges":exchanges,
        "trace_sha256":ferric_bench::sha256_file(&trace_path).unwrap(),"trace_bytes":std::fs::read_to_string(&trace_path).unwrap()})
}

fn pure_request_stall(root: &Path, stages: &mut StageJournal) -> Value {
    // No server is started or contacted. This finite synchronous provider-mode
    // stall exists specifically to test the outer owner independently of an
    // executor/tokio timer and without claiming nested Unix containment.
    let cancel = Arc::new(AtomicBool::new(false));
    let mut watchdog = Watchdog::new(cancel, Duration::from_secs(1), root, "request");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let _ = tokio::time::timeout(Duration::from_secs(1), async {
            stages.record("request_sync_stall_no_engine", "start");
            write_new_json(
                &root.join("request-entered.json"),
                &json!({"synchronous_source_request":true,"engine_started":false}),
            )
            .unwrap();
            std::thread::sleep(Duration::from_secs(20));
        })
        .await;
    });
    watchdog.finish().unwrap();
    json!({"passed":false,"error":"outer owner failed to interrupt synchronous source fixture"})
}

fn read_http(socket: &mut TcpStream) -> Option<Vec<u8>> {
    socket.set_nonblocking(false).ok()?;
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok()?;
    socket
        .set_write_timeout(Some(Duration::from_secs(1)))
        .ok()?;
    let end = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    while Instant::now() < end && bytes.len() < 256 * 1024 {
        let mut buffer = [0; 4096];
        match socket.read(&mut buffer) {
            Ok(0) => return None,
            Ok(read) => bytes.extend_from_slice(&buffer[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => return None,
        }
        if let Some(header_end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if length > 256 * 1024 {
                return None;
            }
            if bytes.len() >= header_end + 4 + length {
                return Some(bytes);
            }
        }
    }
    None
}

#[test]
#[ignore = "finite prepared-engine source fixture, never directly executed from target"]
fn live_budget_engine() {
    let Ok(port) = std::env::var("FERRIC_BUDGET_ENGINE_PORT") else {
        return;
    };
    crate::test_process_containment::ensure_current_process_tree_is_contained().unwrap();
    let port: u16 = port.parse().unwrap();
    let root = PathBuf::from(std::env::var_os("FERRIC_BUDGET_CHILD_ROOT").unwrap());
    let mode = std::env::var("FERRIC_BUDGET_ENGINE_MODE").unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
    listener.set_nonblocking(true).unwrap();
    write_new_json(
        &root.join("engine-listener.json"),
        &json!({"port":port,"pid":std::process::id()}),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let mut socket = match listener.accept() {
            Ok((socket, _)) => socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_) => break,
        };
        let Some(bytes) = read_http(&mut socket) else {
            continue;
        };
        let completion = bytes.starts_with(b"POST /v1/chat/completions ");
        if completion {
            write_new_json(
                &root.join("engine-completion.json"),
                &json!({"request_received":true}),
            )
            .unwrap();
        }
        if mode == "setup-cancel" || completion {
            // Hold the actual request open; the phase cancellation closes it.
            let mut byte = [0; 1];
            while Instant::now() < deadline {
                match socket.read(&mut byte) {
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
        let body = if bytes.starts_with(b"GET /health ") {
            "{}"
        } else {
            r#"{"data":[{"id":"budget-fixture-model"}]}"#
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes());
    }
}
