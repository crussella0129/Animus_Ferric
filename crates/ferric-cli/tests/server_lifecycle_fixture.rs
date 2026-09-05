//! Black-box lifecycle proofs against the real `ferric` executable.
//!
//! The fixture binary is feature-gated and copied to the exact closed-engine
//! filename, so these tests exercise production CLI parsing, process identity,
//! listener ownership, registration publication, adoption, and teardown.

#![cfg(all(
    feature = "lifecycle-fixture",
    any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )
))]

#[cfg(any(windows, target_os = "linux"))]
#[path = "../src/test_process_containment.rs"]
mod test_process_containment;

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
#[cfg(target_os = "linux")]
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{
    Arc, Mutex, MutexGuard, OnceLock,
    mpsc::{self, Receiver, Sender},
};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const SENTINEL_NAME: &str = "unrelated-sentinel.txt";
const BIND_DIAGNOSTIC_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_BIND_DIAGNOSTIC";
const READY_MARKER_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_READY_MARKER";
#[cfg(target_os = "linux")]
const OWNER_PID_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_OWNER_PID";
#[cfg(target_os = "linux")]
const OWNER_START_TICKS_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_OWNER_START_TICKS";
#[cfg(target_os = "linux")]
const OWNER_DEATH_PROBE_MODE_ENV: &str = "FERRIC_LIFECYCLE_OWNER_DEATH_PROBE_MODE";
#[cfg(target_os = "linux")]
const OWNER_DEATH_PROBE_FIXTURE_ENV: &str = "FERRIC_LIFECYCLE_OWNER_DEATH_PROBE_FIXTURE";
#[cfg(target_os = "linux")]
const OWNER_DEATH_PROBE_MODEL_ENV: &str = "FERRIC_LIFECYCLE_OWNER_DEATH_PROBE_MODEL";
#[cfg(target_os = "linux")]
const OWNER_DEATH_PROBE_TOKEN_ENV: &str = "FERRIC_LIFECYCLE_OWNER_DEATH_PROBE_TOKEN";
#[cfg(target_os = "linux")]
const OWNER_DEATH_PROBE_READY_ENV: &str = "FERRIC_LIFECYCLE_OWNER_DEATH_PROBE_READY";
#[cfg(target_os = "linux")]
const OWNER_DEATH_PROBE_PORT_ENV: &str = "FERRIC_LIFECYCLE_OWNER_DEATH_PROBE_PORT";
#[cfg(target_os = "linux")]
const OWNER_DEATH_PROBE_TEST_NAME: &str = "lifecycle_fixture_exits_when_exact_owner_pidfd_signals";
const TAILSCALE_LOCALAPI_TEST_TCP_ENV: &str = "FERRIC_TAILSCALE_LOCALAPI_TEST_TCP";
const TAILSCALE_FQDN: &str = "example-host.tailnet-example.ts.net";
const TAILSCALE_STABLE_NODE_ID: &str = "node-fixture";
const UNRELATED_SERVE_PATH: &str = "/unrelated-kept";
const ADDRESS_IN_USE_DIAGNOSTIC: &[u8] = b"ferric-lifecycle-fixture:address-in-use:v1\n";
const READY_MARKER: &[u8] = b"ferric-lifecycle-fixture:ready:v1\n";
const CLI_TIMEOUT: Duration = Duration::from_secs(30);
const CHILD_EXIT_GRACE: Duration = Duration::from_secs(5);
const FIXTURE_LIFETIME_LIMIT: Duration = Duration::from_secs(90);
const PORT_ATTEMPTS: usize = 3;

fn lifecycle_test_lock() -> MutexGuard<'static, ()> {
    test_process_containment::ensure_current_process_tree_is_contained()
        .expect("install lifecycle-test process containment");
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn canonical_test_start_token(coordinate: u64) -> String {
    assert!(
        coordinate > 0,
        "test start-token coordinate must be positive"
    );

    #[cfg(windows)]
    {
        format!("windows-filetime:{coordinate}")
    }

    #[cfg(all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        format!("linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:{coordinate}")
    }
}

fn ferric_executable() -> &'static str {
    env!("CARGO_BIN_EXE_ferric-lifecycle-test")
}

fn production_ferric_executable() -> &'static str {
    env!("CARGO_BIN_EXE_ferric")
}

fn fixture_executable() -> &'static str {
    env!("CARGO_BIN_EXE_ferric-lifecycle-fixture")
}

fn engine_filename() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

#[cfg(target_os = "linux")]
fn linux_self_start_ticks() -> String {
    linux_process_start_ticks(std::process::id())
        .expect("read lifecycle harness stat")
        .to_string()
}

#[cfg(target_os = "linux")]
fn linux_process_start_ticks(pid: u32) -> std::io::Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close = stat.rfind(')').ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "lifecycle process stat lacked a command delimiter",
        )
    })?;
    stat.get(close + 1..)
        .and_then(|suffix| suffix.split_whitespace().nth(19))
        .and_then(|field| field.parse::<u64>().ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "lifecycle process stat lacked numeric start ticks",
            )
        })
}

#[cfg(target_os = "linux")]
fn linux_process_start_token(pid: u32) -> std::io::Result<String> {
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let boot_id = boot_id.trim();
    if boot_id.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Linux boot ID was empty",
        ));
    }
    let start_ticks = linux_process_start_ticks(pid)?;
    Ok(format!("linux-boot-id:{boot_id};start-ticks:{start_ticks}"))
}

fn configure_fixture_owner_environment(command: &mut Command) {
    #[cfg(target_os = "linux")]
    command
        .env(OWNER_PID_ENV, std::process::id().to_string())
        .env(OWNER_START_TICKS_ENV, linux_self_start_ticks());

    #[cfg(windows)]
    let _ = command;
}

fn copy_fixture_as(bin_dir: &Path, filename: &str) -> PathBuf {
    let executable = bin_dir.join(filename);
    fs::copy(fixture_executable(), &executable).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
    }
    executable
}

fn install_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let bin_dir = root.join("isolated-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let engine = copy_fixture_as(&bin_dir, engine_filename());
    (bin_dir, engine)
}

fn isolated_ferric(workspace: &Path, appdata: &Path, bin_dir: &Path) -> Command {
    let mut command = Command::new(ferric_executable());
    command
        .current_dir(workspace)
        // All config selectors are child-scoped. APPDATA wins on every host,
        // while isolated XDG/HOME values make fallback behavior harmless too.
        .env("APPDATA", appdata)
        .env("XDG_CONFIG_HOME", appdata.join("unused-xdg"))
        .env("HOME", appdata.join("unused-home"))
        // Ferric can resolve only the copied closed-engine fixture.
        .env("PATH", bin_dir);
    test_process_containment::configure_command_parent_death_signal(&mut command)
        .expect("configure Ferric test-child parent-death containment");
    configure_fixture_owner_environment(&mut command);
    command
}

fn isolated_localapi_ferric(
    workspace: &Path,
    appdata: &Path,
    bin_dir: &Path,
    localapi_address: std::net::SocketAddr,
) -> Command {
    let mut command = isolated_ferric(workspace, appdata, bin_dir);
    command.env(
        TAILSCALE_LOCALAPI_TEST_TCP_ENV,
        localapi_address.to_string(),
    );
    command
}

fn isolated_production_ferric(workspace: &Path, appdata: &Path, bin_dir: &Path) -> Command {
    let mut command = Command::new(production_ferric_executable());
    command
        .current_dir(workspace)
        .env("APPDATA", appdata)
        .env("XDG_CONFIG_HOME", appdata.join("unused-xdg"))
        .env("HOME", appdata.join("unused-home"))
        .env("PATH", bin_dir);
    test_process_containment::configure_command_parent_death_signal(&mut command)
        .expect("configure production Ferric test-child parent-death containment");
    configure_fixture_owner_environment(&mut command);
    command
}

fn wait_for_child_exit(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn report_drop_cleanup_failure(context: &str, error: std::io::Error) {
    test_process_containment::abort_on_cleanup_failure(context, error);
}

fn run_cli_output_bounded(label: &str, command: &mut Command) -> Output {
    test_process_containment::output_bounded(command, CLI_TIMEOUT)
        .unwrap_or_else(|error| panic!("run {label} within owned CLI scope: {error}"))
}

fn run_managed_cli_output_bounded(
    label: &str,
    command: &mut Command,
    lifetime: &mut FixtureLifetimeGuard,
) -> Output {
    use std::io::{Seek, SeekFrom};

    let mut stdout = tempfile::tempfile().expect("create managed CLI stdout capture");
    let mut stderr = tempfile::tempfile().expect("create managed CLI stderr capture");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()));
    let child = test_process_containment::ContainedChild::spawn(command)
        .unwrap_or_else(|error| panic!("spawn {label} in retained lifecycle scope: {error}"));
    // `server up` deliberately hands its server to the test's lifecycle owner.
    // Keep the launcher's scope (and unreaped leader identity) through that
    // handoff, including failures before a registration can be decoded.
    lifetime.scopes.push(child);
    let child = lifetime.scopes.last_mut().unwrap();
    let deadline = Instant::now() + CLI_TIMEOUT;
    let status = loop {
        match child.try_wait_leader() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => panic!("{label} did not complete its bounded launcher handoff: {result:?}"),
        }
    };
    stdout.seek(SeekFrom::Start(0)).unwrap();
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    stdout.read_to_end(&mut stdout_bytes).unwrap();
    stderr.read_to_end(&mut stderr_bytes).unwrap();
    Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    }
}

fn assert_success(label: &str, output: Output) {
    assert!(
        output.status.success(),
        "{label} failed ({}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn output_lines(label: &str, stream: &str, bytes: &[u8]) -> Vec<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|error| panic!("{label} {stream} was not UTF-8: {error}"))
        .lines()
        .map(str::to_string)
        .collect()
}

fn assert_failed_output(
    label: &str,
    output: &Output,
    expected_stdout: &[String],
    expected_stderr: &[String],
) {
    assert!(
        !output.status.success(),
        "{label} unexpectedly succeeded:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output_lines(label, "stdout", &output.stdout),
        expected_stdout,
        "{label} stdout did not render the complete blocked contract"
    );
    assert_eq!(
        output_lines(label, "stderr", &output.stderr),
        expected_stderr,
        "{label} stderr did not render the complete blocked contract"
    );
}

fn unused_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn endpoint_is_healthy(port: u16) -> bool {
    let address = format!("127.0.0.1:{port}").parse().unwrap();
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(250)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    stream.read_to_end(&mut response).is_ok() && response.starts_with(b"HTTP/1.1 200 OK\r\n")
}

fn endpoint_is_closed(port: u16) -> bool {
    let address = format!("127.0.0.1:{port}").parse().unwrap();
    TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_err()
}

fn wait_until(mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    predicate()
}

fn marker_matches(path: &Path, expected: &[u8]) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes == expected)
}

fn remove_marker(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!(
            "could not remove fixture marker {}: {error}",
            path.display()
        ),
    }
}

#[derive(PartialEq, Eq)]
enum TreeEntry {
    Directory,
    File(Box<[u8]>),
    Symlink(PathBuf),
}

fn byte_fingerprint(bytes: &[u8]) -> u64 {
    let mut fingerprint = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        fingerprint ^= u64::from(*byte);
        fingerprint = fingerprint.wrapping_mul(0x0000_0100_0000_01b3);
    }
    fingerprint
}

impl fmt::Debug for TreeEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory => formatter.write_str("Directory"),
            Self::File(bytes) => formatter
                .debug_struct("File")
                .field("bytes", &bytes.len())
                .field(
                    "fingerprint",
                    &format_args!("{:#018x}", byte_fingerprint(bytes)),
                )
                .finish(),
            Self::Symlink(target) => formatter.debug_tuple("Symlink").field(target).finish(),
        }
    }
}

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, TreeEntry> {
    fn visit(root: &Path, directory: &Path, snapshot: &mut BTreeMap<PathBuf, TreeEntry>) {
        let mut entries = fs::read_dir(directory)
            .unwrap_or_else(|error| {
                panic!(
                    "could not enumerate snapshot directory {}: {error}",
                    directory.display()
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.file_type().is_symlink() {
                snapshot.insert(relative, TreeEntry::Symlink(fs::read_link(&path).unwrap()));
            } else if metadata.is_dir() {
                snapshot.insert(relative, TreeEntry::Directory);
                visit(root, &path, snapshot);
            } else if metadata.is_file() {
                let bytes = fs::read(&path).unwrap_or_else(|error| {
                    panic!("could not read snapshot file {}: {error}", path.display())
                });
                snapshot.insert(relative, TreeEntry::File(bytes.into_boxed_slice()));
            } else {
                panic!(
                    "unexpected filesystem object in fixture tree: {}",
                    path.display()
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn write_sentinel(directory: &Path, label: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join(SENTINEL_NAME), label).unwrap();
}

fn assert_only_sentinel(directory: &Path, label: &str) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        vec![SENTINEL_NAME],
        "registration, stage, or coordination artifact remained in {}",
        directory.display()
    );
    assert_eq!(
        fs::read_to_string(directory.join(SENTINEL_NAME)).unwrap(),
        label,
        "unrelated sentinel changed in {}",
        directory.display()
    );
}

fn assert_registration_and_sentinel(directory: &Path, label: &str, expected_raw: &[u8]) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    entries.sort();
    let mut expected_entries: Vec<std::ffi::OsString> =
        vec![SENTINEL_NAME.into(), "server.json".into()];
    expected_entries.sort();
    assert_eq!(
        entries,
        expected_entries,
        "registration, stage, or coordination artifacts changed in {}",
        directory.display()
    );
    assert_eq!(
        fs::read_to_string(directory.join(SENTINEL_NAME)).unwrap(),
        label,
        "unrelated sentinel changed in {}",
        directory.display()
    );
    assert_eq!(
        fs::read(directory.join("server.json")).unwrap(),
        expected_raw,
        "registration bytes changed in {}",
        directory.display()
    );
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn initial_tailscale_state() -> serde_json::Value {
    serde_json::json!({
        "TCP": {"443": {"HTTPS": true}},
        "Web": {
            (format!("{TAILSCALE_FQDN}:443")): {
                "Handlers": {
                    (UNRELATED_SERVE_PATH): {
                        "Text": "unrelated handler must survive"
                    }
                }
            }
        },
        "Services": {
            "svc:demo": {
                "TCP": {"9000": {"TCPForward": "127.0.0.1:9"}},
                "Tun": false
            }
        }
    })
}

fn tailscale_status() -> serde_json::Value {
    serde_json::json!({
        "BackendState": "Running",
        "CertDomains": [TAILSCALE_FQDN],
        "Self": {
            "DNSName": format!("{TAILSCALE_FQDN}."),
            "ID": TAILSCALE_STABLE_NODE_ID,
            "NodeID": "node-id-fixture",
            "CapMap": {"https": null}
        }
    })
}

#[derive(Debug, Clone)]
struct LocalApiRequestLog {
    connection: u64,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    if_match: Option<String>,
    body: Vec<u8>,
    journal_on_post: Option<JournalPostLog>,
}

#[derive(Debug, Clone)]
struct JournalPostLog {
    mirrors_equal: bool,
    schema_version: Option<u64>,
    tailscale: Option<bool>,
    ownership_version: Option<u64>,
    stable_node_id: Option<String>,
    fqdn: Option<String>,
    https_port: Option<u64>,
    mount_path: Option<String>,
    proxy_target: Option<String>,
    remote_base_url: Option<String>,
    before_status_sha256: Option<String>,
    tcp_map_preexisting: Option<bool>,
    tcp_https_preexisting: Option<bool>,
    web_map_preexisting: Option<bool>,
    web_host_preexisting: Option<bool>,
    apply_confirmed: Option<bool>,
    target_healthy: bool,
}

struct FakeLocalApiState {
    serve_config: serde_json::Value,
    requests: Vec<LocalApiRequestLog>,
    local_journal: PathBuf,
    global_journal: PathBuf,
}

struct FakeLocalApi {
    address: std::net::SocketAddr,
    state: Arc<Mutex<FakeLocalApiState>>,
    shutdown: Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl FakeLocalApi {
    fn start(
        serve_config: serde_json::Value,
        local_journal: PathBuf,
        global_journal: PathBuf,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake LocalAPI");
        let address = listener.local_addr().expect("fake LocalAPI address");
        let state = Arc::new(Mutex::new(FakeLocalApiState {
            serve_config,
            requests: Vec::new(),
            local_journal,
            global_journal,
        }));
        let worker_state = Arc::clone(&state);
        let (shutdown, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            run_fake_localapi(listener, worker_state, receiver);
        });
        Self {
            address,
            state,
            shutdown,
            worker: Some(worker),
        }
    }

    fn address(&self) -> std::net::SocketAddr {
        self.address
    }

    fn serve_config(&self) -> serde_json::Value {
        self.state.lock().unwrap().serve_config.clone()
    }

    fn requests(&self) -> Vec<LocalApiRequestLog> {
        self.state.lock().unwrap().requests.clone()
    }

    fn stop(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        let _ = self.shutdown.send(());
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("fake LocalAPI worker joined");
        }
    }
}

impl Drop for FakeLocalApi {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

fn run_fake_localapi(
    listener: TcpListener,
    state: Arc<Mutex<FakeLocalApiState>>,
    shutdown: Receiver<()>,
) {
    let mut connection = 0_u64;
    loop {
        let (mut stream, _) = listener.accept().expect("accept fake LocalAPI connection");
        if shutdown.try_recv().is_ok() {
            break;
        }
        connection += 1;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("set fake LocalAPI read timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .expect("set fake LocalAPI write timeout");
        while let Some(request) = read_localapi_request(&mut stream)
            .unwrap_or_else(|error| panic!("read fake LocalAPI request: {error}"))
        {
            serve_localapi_request(&mut stream, connection, request, &state);
        }
    }
}

struct LocalApiRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    if_match: Option<String>,
    body: Vec<u8>,
}

fn read_localapi_request(stream: &mut TcpStream) -> std::io::Result<Option<LocalApiRequest>> {
    const HEADER_LIMIT: usize = 64 * 1024;
    const BODY_LIMIT: usize = 1024 * 1024;
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) if head.is_empty() => return Ok(None),
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "request headers ended early",
                ));
            }
            Ok(_) => {
                head.push(byte[0]);
                if head.len() > HEADER_LIMIT {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "request headers exceed fixture limit",
                    ));
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && head.is_empty() =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
    }

    let text = std::str::from_utf8(&head)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 headers"))?;
    let mut lines = text[..text.len() - 4].split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split_whitespace();
    let method = request_line.next().unwrap_or_default().to_string();
    let path = request_line.next().unwrap_or_default().to_string();
    if request_line.next() != Some("HTTP/1.1") || request_line.next().is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid HTTP/1.1 request line",
        ));
    }
    let mut content_length = None;
    let mut if_match = None;
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "malformed request header")
        })?;
        let value = value.trim();
        headers.push((name.to_string(), value.to_string()));
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "duplicate Content-Length",
                ));
            }
            content_length = Some(value.parse::<usize>().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid Content-Length")
            })?);
        } else if name.eq_ignore_ascii_case("if-match")
            && if_match.replace(value.to_string()).is_some()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "duplicate If-Match",
            ));
        }
    }
    let content_length = content_length.unwrap_or(0);
    if content_length > BODY_LIMIT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request body exceeds fixture limit",
        ));
    }
    let mut body = vec![0_u8; content_length];
    stream.read_exact(&mut body)?;
    Ok(Some(LocalApiRequest {
        method,
        path,
        headers,
        if_match,
        body,
    }))
}

fn journal_post_log(local_journal: &Path, global_journal: &Path) -> JournalPostLog {
    let local = fs::read(local_journal);
    let global = fs::read(global_journal);
    let mirrors_equal = matches!((&local, &global), (Ok(local), Ok(global)) if local == global);
    let parsed = local
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok());
    let ownership = parsed
        .as_ref()
        .and_then(|runfile| runfile.get("tailscale_serve"));
    let string = |field: &str| {
        ownership
            .and_then(|ownership| ownership.get(field))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let proxy_target = string("proxy_target");
    let target_healthy = proxy_target
        .as_deref()
        .and_then(|target| target.strip_prefix("http://127.0.0.1:"))
        .and_then(|port| port.parse::<u16>().ok())
        .is_some_and(endpoint_is_healthy);
    JournalPostLog {
        mirrors_equal,
        schema_version: parsed
            .as_ref()
            .and_then(|runfile| runfile.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        tailscale: parsed
            .as_ref()
            .and_then(|runfile| runfile.get("tailscale"))
            .and_then(serde_json::Value::as_bool),
        ownership_version: ownership
            .and_then(|ownership| ownership.get("version"))
            .and_then(serde_json::Value::as_u64),
        stable_node_id: string("stable_node_id"),
        fqdn: string("fqdn"),
        https_port: ownership
            .and_then(|ownership| ownership.get("https_port"))
            .and_then(serde_json::Value::as_u64),
        mount_path: string("mount_path"),
        proxy_target,
        remote_base_url: string("remote_base_url"),
        before_status_sha256: string("before_status_sha256"),
        tcp_map_preexisting: ownership
            .and_then(|ownership| ownership.get("tcp_map_preexisting"))
            .and_then(serde_json::Value::as_bool),
        tcp_https_preexisting: ownership
            .and_then(|ownership| ownership.get("tcp_https_preexisting"))
            .and_then(serde_json::Value::as_bool),
        web_map_preexisting: ownership
            .and_then(|ownership| ownership.get("web_map_preexisting"))
            .and_then(serde_json::Value::as_bool),
        web_host_preexisting: ownership
            .and_then(|ownership| ownership.get("web_host_preexisting"))
            .and_then(serde_json::Value::as_bool),
        apply_confirmed: ownership
            .and_then(|ownership| ownership.get("apply_confirmed"))
            .and_then(serde_json::Value::as_bool),
        target_healthy,
    }
}

fn serve_localapi_request(
    stream: &mut TcpStream,
    connection: u64,
    request: LocalApiRequest,
    state: &Arc<Mutex<FakeLocalApiState>>,
) {
    let journal_on_post = (request.method == "POST").then(|| {
        let state = state.lock().unwrap();
        journal_post_log(&state.local_journal, &state.global_journal)
    });
    state.lock().unwrap().requests.push(LocalApiRequestLog {
        connection,
        method: request.method.clone(),
        path: request.path.clone(),
        headers: request.headers.clone(),
        if_match: request.if_match.clone(),
        body: request.body.clone(),
        journal_on_post,
    });

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/localapi/v0/status?peers=false") => write_localapi_response(
            stream,
            200,
            &serde_json::to_vec(&tailscale_status()).unwrap(),
            None,
        ),
        ("GET", "/localapi/v0/serve-config") => {
            let raw = serde_json::to_vec(&state.lock().unwrap().serve_config).unwrap();
            let etag = hex::encode(Sha256::digest(&raw));
            write_localapi_response(stream, 200, &raw, Some(&etag));
        }
        ("POST", "/localapi/v0/serve-config") => {
            let mut state = state.lock().unwrap();
            let current_raw = serde_json::to_vec(&state.serve_config).unwrap();
            let current_etag = hex::encode(Sha256::digest(&current_raw));
            if request.if_match.as_deref() != Some(current_etag.as_str()) {
                drop(state);
                write_localapi_response(stream, 412, br#"{"error":"precondition failed"}"#, None);
                return;
            }
            let replacement: serde_json::Value = serde_json::from_slice(&request.body)
                .expect("fake LocalAPI received valid JSON CAS body");
            state.serve_config = replacement;
            drop(state);
            write_localapi_response(stream, 200, b"{}", None);
        }
        _ => write_localapi_response(stream, 404, br#"{"error":"not found"}"#, None),
    }
}

fn write_localapi_response(stream: &mut TcpStream, status: u16, body: &[u8], etag: Option<&str>) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        412 => "Precondition Failed",
        _ => "Fixture Error",
    };
    let etag_header = etag.map_or_else(String::new, |etag| format!("ETag: {etag}\r\n"));
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nTailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\nContent-Type: application/json\r\n{etag_header}Content-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    )
    .expect("write fake LocalAPI response headers");
    stream
        .write_all(body)
        .expect("write fake LocalAPI response body");
    stream.flush().expect("flush fake LocalAPI response");
}

#[cfg(windows)]
mod native_process {
    use std::ffi::c_void;
    use std::io;
    use std::mem::MaybeUninit;

    type Handle = *mut c_void;
    const PROCESS_TERMINATE: u32 = 0x0001;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = u32::MAX;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FileTime {
        low_date_time: u32,
        high_date_time: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> Handle;
        fn TerminateProcess(process: Handle, exit_code: u32) -> i32;
        fn WaitForSingleObject(object: Handle, milliseconds: u32) -> u32;
        fn CloseHandle(object: Handle) -> i32;
        fn GetProcessTimes(
            process: Handle,
            creation_time: *mut FileTime,
            exit_time: *mut FileTime,
            kernel_time: *mut FileTime,
            user_time: *mut FileTime,
        ) -> i32;
    }

    /// One retained process-object handle. PID reuse cannot retarget it.
    pub struct ExactProcess {
        handle: Handle,
    }

    impl ExactProcess {
        pub fn acquire(pid: u32, expected_start_token: &str) -> io::Result<Self> {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_TERMINATE | SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let process = Self { handle };
            if !process.active()? {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "retained process exited before creation-token validation",
                ));
            }
            let observed_start_token = process.start_token()?;
            if !process.active()? {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "retained process exited during creation-token validation",
                ));
            }
            if observed_start_token != expected_start_token {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "published start token {expected_start_token:?} does not match retained process token {observed_start_token:?}"
                    ),
                ));
            }
            Ok(process)
        }

        fn active(&self) -> io::Result<bool> {
            match unsafe { WaitForSingleObject(self.handle, 0) } {
                WAIT_OBJECT_0 => Ok(false),
                WAIT_TIMEOUT => Ok(true),
                WAIT_FAILED => Err(io::Error::last_os_error()),
                state => Err(io::Error::other(format!(
                    "unexpected retained process wait state {state}"
                ))),
            }
        }

        fn start_token(&self) -> io::Result<String> {
            let mut creation = MaybeUninit::<FileTime>::uninit();
            let mut exit = MaybeUninit::<FileTime>::uninit();
            let mut kernel = MaybeUninit::<FileTime>::uninit();
            let mut user = MaybeUninit::<FileTime>::uninit();
            if unsafe {
                GetProcessTimes(
                    self.handle,
                    creation.as_mut_ptr(),
                    exit.as_mut_ptr(),
                    kernel.as_mut_ptr(),
                    user.as_mut_ptr(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let creation = unsafe { creation.assume_init() };
            let filetime =
                (u64::from(creation.high_date_time) << 32) | u64::from(creation.low_date_time);
            Ok(format!("windows-filetime:{filetime}"))
        }

        pub fn running(&self) -> bool {
            // An observation failure is not proof of exit. Keep cleanup and
            // the assertion fail-closed on the retained exact object.
            self.active().unwrap_or(true)
        }

        pub fn exited(&self) -> io::Result<bool> {
            self.active().map(|active| !active)
        }

        pub fn reap_after_exit(&self) -> io::Result<()> {
            if self.exited()? {
                Ok(())
            } else {
                Err(io::Error::other("retained Windows fixture has not exited"))
            }
        }

        pub fn terminate_for_cleanup(&self) -> io::Result<()> {
            if !self.active()? {
                return Ok(());
            }
            if unsafe { TerminateProcess(self.handle, 1) } == 0 {
                let terminate_error = io::Error::last_os_error();
                if self.active()? {
                    return Err(terminate_error);
                }
                return Ok(());
            }
            match unsafe { WaitForSingleObject(self.handle, 5_000) } {
                WAIT_OBJECT_0 => Ok(()),
                WAIT_TIMEOUT => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for retained process termination",
                )),
                WAIT_FAILED => Err(io::Error::last_os_error()),
                state => Err(io::Error::other(format!(
                    "unexpected retained process cleanup wait state {state}"
                ))),
            }
        }
    }

    impl Drop for ExactProcess {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod native_process {
    use std::ffi::{c_int, c_long, c_void};
    use std::fs;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    const SYS_PIDFD_SEND_SIGNAL: c_long = 424;
    const SYS_PIDFD_OPEN: c_long = 434;
    const POLLIN: i16 = 0x0001;
    const POLLHUP: i16 = 0x0010;
    const SIGKILL: c_int = 9;

    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn syscall(number: c_long, ...) -> c_long;
        fn poll(descriptors: *mut PollFd, count: usize, timeout_ms: c_int) -> c_int;
    }

    /// One retained pidfd. PID reuse cannot retarget it, and polling it treats
    /// an exited-but-unreaped process as exited rather than `/proc`-live.
    pub struct ExactProcess {
        pidfd: OwnedFd,
    }

    impl ExactProcess {
        pub fn acquire(pid: u32, expected_start_token: &str) -> io::Result<Self> {
            if pid == 0 || pid > c_int::MAX as u32 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "published PID is outside the pidfd range",
                ));
            }
            let raw = unsafe { syscall(SYS_PIDFD_OPEN, pid as c_int, 0_u32) };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let pidfd = unsafe { OwnedFd::from_raw_fd(raw as c_int) };
            let process = Self { pidfd };

            // All identity coordinates below are PID-indexed. Active pidfd
            // checks bracket the complete read so exit/reuse cannot pair a
            // replacement's /proc facts with this retained process object.
            if !process.active()? {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "retained process exited before start-token validation",
                ));
            }
            let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
            let boot_id = boot_id.trim();
            if boot_id.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Linux boot ID was empty",
                ));
            }
            let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
            let close = stat.rfind(')').ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "process stat did not contain a closing command delimiter",
                )
            })?;
            let start_ticks = stat
                .get(close + 1..)
                .and_then(|suffix| suffix.split_whitespace().nth(19))
                .and_then(|field| field.parse::<u64>().ok())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "process stat did not contain numeric start ticks",
                    )
                })?;
            let observed_start_token = format!("linux-boot-id:{boot_id};start-ticks:{start_ticks}");
            if !process.active()? {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "retained process exited during start-token validation",
                ));
            }
            if observed_start_token != expected_start_token {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "published start token {expected_start_token:?} does not match retained process token {observed_start_token:?}"
                    ),
                ));
            }
            Ok(process)
        }

        fn poll_exit(&self, timeout_ms: c_int) -> io::Result<bool> {
            let mut descriptor = PollFd {
                fd: self.pidfd.as_raw_fd(),
                events: POLLIN,
                revents: 0,
            };
            let result = unsafe { poll(&mut descriptor, 1, timeout_ms) };
            if result == 0 {
                return Ok(false);
            }
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            if descriptor.revents & !(POLLIN | POLLHUP) != 0 {
                Err(io::Error::other(format!(
                    "retained pidfd returned invalid poll events {:#x}",
                    descriptor.revents
                )))
            } else if descriptor.revents & (POLLIN | POLLHUP) != 0 {
                Ok(true)
            } else {
                Err(io::Error::other(format!(
                    "retained pidfd returned ambiguous poll events {:#x}",
                    descriptor.revents
                )))
            }
        }

        fn active(&self) -> io::Result<bool> {
            self.poll_exit(0).map(|exited| !exited)
        }

        pub fn running(&self) -> bool {
            // A poll failure is not proof of exit. Keep cleanup and the
            // assertion fail-closed on the retained exact object.
            self.active().unwrap_or(true)
        }

        pub fn exited(&self) -> io::Result<bool> {
            self.active().map(|active| !active)
        }

        /// Reap only this retained process. A fixture still owned by its
        /// source supervisor may be reaped there; POLLHUP proves that case.
        /// Unlike numeric waitpid, P_PIDFD cannot target a reused process ID.
        pub fn reap_after_exit(&self) -> io::Result<()> {
            let deadline = std::time::Instant::now() + super::CHILD_EXIT_GRACE;
            loop {
                let mut descriptor = PollFd {
                    fd: self.pidfd.as_raw_fd(),
                    events: POLLIN,
                    revents: 0,
                };
                let polled = unsafe { poll(&mut descriptor, 1, 0) };
                if polled < 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(error);
                }
                if descriptor.revents & !(POLLIN | POLLHUP) != 0 {
                    return Err(io::Error::other(format!(
                        "retained pidfd returned invalid reap events {:#x}",
                        descriptor.revents
                    )));
                }
                if descriptor.revents & POLLHUP != 0 {
                    return Ok(());
                }
                let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let waited = unsafe {
                    libc::waitid(
                        libc::P_PIDFD,
                        self.pidfd.as_raw_fd() as libc::id_t,
                        &mut status,
                        libc::WEXITED | libc::WNOHANG,
                    )
                };
                if waited == 0 && unsafe { status.si_pid() } != 0 {
                    return Ok(());
                }
                if waited != 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ECHILD)
                        && error.kind() != io::ErrorKind::Interrupted
                    {
                        return Err(error);
                    }
                }
                if std::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "exact fixture exited but neither source reaping nor external reaping was proved",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        pub fn terminate_for_cleanup(&self) -> io::Result<()> {
            if !self.active()? {
                return Ok(());
            }
            let signal_result = unsafe {
                syscall(
                    SYS_PIDFD_SEND_SIGNAL,
                    self.pidfd.as_raw_fd(),
                    SIGKILL,
                    std::ptr::null::<c_void>(),
                    0_u32,
                )
            };
            if signal_result < 0 {
                let signal_error = io::Error::last_os_error();
                if self.active()? {
                    return Err(signal_error);
                }
                return Ok(());
            }
            if self.poll_exit(5_000)? {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for retained process termination",
                ))
            }
        }
    }
}

/// Cleans an up-launched fixture if a later assertion fails. The exact OS
/// process object is acquired once from the just-published PID and retained;
/// neither liveness checks nor cleanup reopen that numeric PID.
struct ExternalProcessGuard {
    process: native_process::ExactProcess,
    armed: bool,
    reap: bool,
}

impl ExternalProcessGuard {
    fn acquire(pid: u32, expected_start_token: &str) -> std::io::Result<Self> {
        Ok(Self {
            process: native_process::ExactProcess::acquire(pid, expected_start_token)?,
            armed: true,
            reap: true,
        })
    }

    #[cfg(target_os = "linux")]
    fn observe_child(pid: u32, expected_start_token: &str) -> std::io::Result<Self> {
        let mut guard = Self::acquire(pid, expected_start_token)?;
        // Its ContainedChild scope owns the direct leader's reap obligation.
        guard.reap = false;
        Ok(guard)
    }

    fn running(&self) -> bool {
        self.process.running()
    }

    fn disarm_after_exit(&mut self) -> std::io::Result<()> {
        if !self.process.exited()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "cannot disarm cleanup for a retained process that is still running",
            ));
        }
        if self.reap {
            self.process.reap_after_exit()?;
        }
        self.armed = false;
        Ok(())
    }

    fn terminate_and_disarm(&mut self) -> std::io::Result<()> {
        self.process.terminate_for_cleanup()?;
        self.disarm_after_exit()
    }
}

impl Drop for ExternalProcessGuard {
    fn drop(&mut self) {
        if self.armed
            && let Err(first_error) = self.terminate_and_disarm()
            && let Err(retry_error) = self.terminate_and_disarm()
        {
            report_drop_cleanup_failure(
                "could not terminate retained fixture process in cleanup guard",
                std::io::Error::other(format!(
                    "first attempt: {first_error}; retry: {retry_error}"
                )),
            );
        }
    }
}

/// Test-only self-termination control for any fixture a broken refusal path
/// might unexpectedly spawn. Removing the private token makes that exact
/// fixture exit without turning a numeric PID into cleanup authority.
struct FixtureLifetimeGuard {
    token: PathBuf,
    port: u16,
    watchdog_cancel: Option<Sender<()>>,
    watchdog: Option<thread::JoinHandle<()>>,
    armed: bool,
    scopes: Vec<test_process_containment::ContainedChild>,
}

impl FixtureLifetimeGuard {
    fn create(token: PathBuf, port: u16) -> Self {
        fs::write(&token, b"fixture may run only while this token exists").unwrap();
        let watched_token = token.clone();
        let (watchdog_cancel, cancel) = mpsc::channel();
        let watchdog = thread::spawn(move || {
            if cancel.recv_timeout(FIXTURE_LIFETIME_LIMIT).is_err() {
                let _ = fs::remove_file(watched_token);
            }
        });
        Self {
            token,
            port,
            watchdog_cancel: Some(watchdog_cancel),
            watchdog: Some(watchdog),
            armed: true,
            scopes: Vec::new(),
        }
    }

    fn token(&self) -> &Path {
        &self.token
    }

    fn cleanup(&mut self) -> std::io::Result<()> {
        let mut first_error = match fs::remove_file(&self.token) {
            Ok(()) => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => Some(std::io::Error::new(
                error.kind(),
                format!(
                    "could not remove fixture lifetime token {}: {error}",
                    self.token.display()
                ),
            )),
        };
        if let Some(cancel) = self.watchdog_cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(watchdog) = self.watchdog.take()
            && watchdog.join().is_err()
            && first_error.is_none()
        {
            first_error = Some(std::io::Error::other(
                "fixture lifetime watchdog panicked during cleanup",
            ));
        }
        for scope in &mut self.scopes {
            if let Err(error) = scope.terminate_and_reap()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        self.scopes.clear();
        if !wait_until(|| endpoint_is_closed(self.port)) && first_error.is_none() {
            first_error = Some(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "fixture listener 127.0.0.1:{} remained open after token removal",
                    self.port
                ),
            ));
        }
        first_error.map_or(Ok(()), Err)
    }

    fn finish(&mut self) -> std::io::Result<()> {
        self.cleanup()?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for FixtureLifetimeGuard {
    fn drop(&mut self) {
        if self.armed
            && let Err(first_error) = self.cleanup()
            && let Err(retry_error) = self.cleanup()
        {
            report_drop_cleanup_failure(
                "could not clean fixture lifetime guard",
                std::io::Error::other(format!(
                    "first attempt: {first_error}; retry: {retry_error}"
                )),
            );
        }
    }
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn child_mut(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }

    fn disarm_reaped(&mut self) -> std::io::Result<()> {
        let Some(child) = self.0.as_mut() else {
            return Ok(());
        };
        if child.try_wait()?.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "cannot disarm cleanup for a child that has not exited",
            ));
        }
        let _ = self.0.take();
        Ok(())
    }

    fn terminate_and_reap(&mut self) -> std::io::Result<()> {
        let Some(child) = self.0.as_mut() else {
            return Ok(());
        };
        let mut cleanup_diagnostics = Vec::new();
        let exited = match child.try_wait() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(error) => {
                cleanup_diagnostics.push(format!("initial exit observation failed: {error}"));
                false
            }
        };
        if !exited && let Err(error) = child.kill() {
            cleanup_diagnostics.push(format!("termination failed: {error}"));
        }
        match wait_for_child_exit(child, CHILD_EXIT_GRACE) {
            Ok(Some(_)) => {
                let _ = self.0.take();
                Ok(())
            }
            Ok(None) => {
                cleanup_diagnostics.push("timed out reaping child after termination".to_string());
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    cleanup_diagnostics.join("; "),
                ))
            }
            Err(error) => {
                cleanup_diagnostics.push(format!("exit wait failed: {error}"));
                Err(std::io::Error::other(cleanup_diagnostics.join("; ")))
            }
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Err(first_error) = self.terminate_and_reap()
            && let Err(retry_error) = self.terminate_and_reap()
        {
            report_drop_cleanup_failure(
                "could not terminate and reap child in cleanup guard",
                std::io::Error::other(format!(
                    "first attempt: {first_error}; retry: {retry_error}"
                )),
            );
        }
    }
}

#[cfg(target_os = "linux")]
struct OwnerDeathCoordinate {
    owner_pid: u32,
    owner_start_token: String,
    fixture_pid: u32,
    fixture_start_token: String,
    port: u16,
}

#[cfg(target_os = "linux")]
fn owner_death_probe_path(environment: &str) -> std::io::Result<PathBuf> {
    std::env::var_os(environment)
        .map(PathBuf::from)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("owner-death probe requires {environment}"),
            )
        })
}

#[cfg(target_os = "linux")]
fn atomic_publish_owner_death_coordinate(
    path: &Path,
    coordinate: &OwnerDeathCoordinate,
) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "owner-death coordinate path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let filename = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "owner-death coordinate path has no filename",
        )
    })?;
    let staged = parent.join(format!(
        ".{}.{}.staged",
        filename.to_string_lossy(),
        std::process::id()
    ));
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "owner_pid": coordinate.owner_pid,
        "owner_start_token": coordinate.owner_start_token,
        "fixture_pid": coordinate.fixture_pid,
        "fixture_start_token": coordinate.fixture_start_token,
        "port": coordinate.port,
    }))
    .map_err(std::io::Error::other)?;
    let publish = (|| -> std::io::Result<()> {
        let mut file = File::create(&staged)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&staged, path)
    })();
    if publish.is_err() {
        let _ = fs::remove_file(&staged);
    }
    publish
}

#[cfg(target_os = "linux")]
fn read_owner_death_coordinate(path: &Path) -> std::io::Result<OwnerDeathCoordinate> {
    let raw = fs::read(path)?;
    let value: serde_json::Value = serde_json::from_slice(&raw).map_err(std::io::Error::other)?;
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "owner-death coordinate has an unsupported schema version",
        ));
    }
    let numeric = |name: &str| -> std::io::Result<u64> {
        value
            .get(name)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("owner-death coordinate lacks numeric {name}"),
                )
            })
    };
    let text = |name: &str| -> std::io::Result<String> {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("owner-death coordinate lacks nonempty {name}"),
                )
            })
    };
    let owner_pid = u32::try_from(numeric("owner_pid")?).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "owner-death coordinate owner PID is out of range",
        )
    })?;
    let fixture_pid = u32::try_from(numeric("fixture_pid")?).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "owner-death coordinate fixture PID is out of range",
        )
    })?;
    let port = u16::try_from(numeric("port")?).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "owner-death coordinate port is out of range",
        )
    })?;
    if owner_pid <= 1 || fixture_pid <= 1 || port == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "owner-death coordinate contains an invalid process or listener coordinate",
        ));
    }
    Ok(OwnerDeathCoordinate {
        owner_pid,
        owner_start_token: text("owner_start_token")?,
        fixture_pid,
        fixture_start_token: text("fixture_start_token")?,
        port,
    })
}

#[cfg(target_os = "linux")]
fn run_owner_death_probe() -> std::io::Result<()> {
    let fixture = owner_death_probe_path(OWNER_DEATH_PROBE_FIXTURE_ENV)?;
    let model = owner_death_probe_path(OWNER_DEATH_PROBE_MODEL_ENV)?;
    let lifetime_token = owner_death_probe_path(OWNER_DEATH_PROBE_TOKEN_ENV)?;
    let ready = owner_death_probe_path(OWNER_DEATH_PROBE_READY_ENV)?;
    let port = std::env::var(OWNER_DEATH_PROBE_PORT_ENV)
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("owner-death probe requires {OWNER_DEATH_PROBE_PORT_ENV}"),
            )
        })?
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "owner-death probe port must be nonzero and numeric",
            )
        })?;
    if !fixture.is_file() || !model.is_file() || !lifetime_token.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "owner-death probe inputs must be regular files",
        ));
    }

    let mut command = Command::new(fixture);
    command
        .args([
            "-m",
            model.to_str().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "owner-death probe model path is not UTF-8",
                )
            })?,
            "-c",
            "4096",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN", &lifetime_token)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // This omission is deliberate: the fixture's retained exact-owner pidfd,
    // not PR_SET_PDEATHSIG or a process-group kill, is the mechanism under test.
    configure_fixture_owner_environment(&mut command);
    let mut fixture = command.spawn().map(ChildGuard::new)?;
    let fixture_pid = fixture.child_mut().id();
    let readiness_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match fixture.child_mut().try_wait() {
            Ok(Some(status)) => {
                fixture.disarm_reaped()?;
                return Err(std::io::Error::other(format!(
                    "lifecycle fixture exited before owner-death readiness: {status}"
                )));
            }
            Ok(None) => {}
            Err(error) => {
                let cleanup = fixture.terminate_and_reap();
                return Err(std::io::Error::other(format!(
                    "could not observe lifecycle fixture before readiness: {error}; checked cleanup={cleanup:?}"
                )));
            }
        }
        if endpoint_is_healthy(port) {
            break;
        }
        if Instant::now() >= readiness_deadline {
            let cleanup = fixture.terminate_and_reap();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "lifecycle fixture exceeded the owner-death readiness deadline; checked cleanup={cleanup:?}"
                ),
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }

    let owner_pid = std::process::id();
    let coordinate = OwnerDeathCoordinate {
        owner_pid,
        owner_start_token: linux_process_start_token(owner_pid)?,
        fixture_pid,
        fixture_start_token: linux_process_start_token(fixture_pid)?,
        port,
    };
    atomic_publish_owner_death_coordinate(&ready, &coordinate)?;

    // The root test normally terminates this exact owner. This fallback is
    // intentionally longer than every assertion window and still guarantees
    // source-owned cleanup if the root test stops making progress.
    let fallback_deadline = Instant::now() + Duration::from_secs(180);
    loop {
        match fixture.child_mut().try_wait() {
            Ok(Some(status)) => {
                fixture.disarm_reaped()?;
                return Err(std::io::Error::other(format!(
                    "lifecycle fixture exited while its exact owner remained alive: {status}"
                )));
            }
            Ok(None) => {}
            Err(error) => {
                let cleanup = fixture.terminate_and_reap();
                return Err(std::io::Error::other(format!(
                    "could not observe lifecycle fixture during owner-death probe: {error}; checked cleanup={cleanup:?}"
                )));
            }
        }
        if Instant::now() >= fallback_deadline {
            let cleanup = fixture.terminate_and_reap();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "owner-death probe reached its 180-second fallback; checked fixture cleanup={cleanup:?}"
                ),
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(target_os = "linux")]
fn wait_for_owner_death_coordinate(
    owner: &mut test_process_containment::ContainedChild,
    ready: &Path,
) -> Result<OwnerDeathCoordinate, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ready.exists() {
            return read_owner_death_coordinate(ready)
                .map_err(|error| format!("could not read atomic owner-death readiness: {error}"));
        }
        match owner.try_wait_leader() {
            Ok(Some(status)) => {
                owner.terminate_and_reap().map_err(|error| {
                    format!("could not confirm failed owner was reaped: {error}")
                })?;
                return Err(format!(
                    "sacrificial owner exited before publishing readiness: {status}"
                ));
            }
            Ok(None) => {}
            Err(error) => {
                return Err(format!(
                    "could not observe sacrificial owner before readiness: {error}"
                ));
            }
        }
        if Instant::now() >= deadline {
            return Err("sacrificial owner exceeded the readiness deadline".to_string());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

struct RunningFixture {
    port: u16,
    lifetime: FixtureLifetimeGuard,
    child: ChildGuard,
}

fn launch_managed_fixture_with_retry(
    root: &Path,
    workspace: &Path,
    appdata: &Path,
    bin_dir: &Path,
    model: &Path,
) -> (u16, FixtureLifetimeGuard) {
    for attempt in 1..=PORT_ATTEMPTS {
        let port = unused_port();
        let token = root.join(format!("managed-fixture-{attempt}.lifetime"));
        let bind_diagnostic = root.join(format!("managed-fixture-{attempt}.bind"));
        let mut lifetime = FixtureLifetimeGuard::create(token, port);
        let mut command = isolated_ferric(workspace, appdata, bin_dir);
        command
            .env("FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN", lifetime.token())
            .env(BIND_DIAGNOSTIC_ENV, &bind_diagnostic)
            .args([
                "server",
                "up",
                "--model",
                model.to_str().unwrap(),
                "--ctx",
                "4096",
                "--port",
                &port.to_string(),
            ]);
        let output =
            run_managed_cli_output_bounded("real `ferric server up`", &mut command, &mut lifetime);
        let address_in_use = marker_matches(&bind_diagnostic, ADDRESS_IN_USE_DIAGNOSTIC);
        if output.status.success() {
            assert!(
                !bind_diagnostic.exists(),
                "managed fixture reported a bind failure despite successful launch"
            );
            return (port, lifetime);
        }

        lifetime
            .finish()
            .unwrap_or_else(|error| panic!("could not clean failed managed fixture: {error}"));
        remove_marker(&bind_diagnostic);
        if address_in_use && attempt < PORT_ATTEMPTS {
            continue;
        }
        if address_in_use {
            panic!(
                "real `ferric server up` exhausted {PORT_ATTEMPTS} diagnosed address-in-use attempts ({}):\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        panic!(
            "real `ferric server up` failed without the fixture's exact address-in-use diagnostic ({}):\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    unreachable!("the diagnosed bind retry loop always returns or panics")
}

fn launch_tailscale_managed_fixture_with_retry(
    root: &Path,
    workspace: &Path,
    appdata: &Path,
    bin_dir: &Path,
    model: &Path,
    localapi_address: std::net::SocketAddr,
) -> (u16, FixtureLifetimeGuard, Output) {
    for attempt in 1..=PORT_ATTEMPTS {
        let port = unused_port();
        let token = root.join(format!("tailscale-managed-fixture-{attempt}.lifetime"));
        let bind_diagnostic = root.join(format!("tailscale-managed-fixture-{attempt}.bind"));
        let mut lifetime = FixtureLifetimeGuard::create(token, port);
        let mut command = isolated_localapi_ferric(workspace, appdata, bin_dir, localapi_address);
        command
            .env("FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN", lifetime.token())
            .env(BIND_DIAGNOSTIC_ENV, &bind_diagnostic)
            .args([
                "server",
                "up",
                "--tailscale",
                "--model",
                model.to_str().unwrap(),
                "--ctx",
                "4096",
                "--port",
                &port.to_string(),
            ]);
        let output = run_managed_cli_output_bounded(
            "real `ferric server up --tailscale`",
            &mut command,
            &mut lifetime,
        );
        let address_in_use = marker_matches(&bind_diagnostic, ADDRESS_IN_USE_DIAGNOSTIC);
        if output.status.success() {
            assert!(
                !bind_diagnostic.exists(),
                "Tailscale managed fixture reported a bind failure despite successful launch"
            );
            return (port, lifetime, output);
        }

        lifetime.finish().unwrap_or_else(|error| {
            panic!("could not clean failed Tailscale managed fixture: {error}")
        });
        remove_marker(&bind_diagnostic);
        if address_in_use && attempt < PORT_ATTEMPTS {
            continue;
        }
        if address_in_use {
            panic!(
                "real `ferric server up --tailscale` exhausted {PORT_ATTEMPTS} diagnosed address-in-use attempts ({}):\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        panic!(
            "real `ferric server up --tailscale` failed without the fixture's exact address-in-use diagnostic ({}):\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    unreachable!("the diagnosed Tailscale bind retry loop always returns or panics")
}

fn launch_direct_fixture_with_retry(
    root: &Path,
    workspace: &Path,
    engine: &Path,
    model: &Path,
    label: &str,
) -> RunningFixture {
    for attempt in 1..=PORT_ATTEMPTS {
        let port = unused_port();
        let token = root.join(format!("{label}-{attempt}.lifetime"));
        let bind_diagnostic = root.join(format!("{label}-{attempt}.bind"));
        let ready_marker = root.join(format!("{label}-{attempt}.ready"));
        let mut lifetime = FixtureLifetimeGuard::create(token, port);
        let mut child = Command::new(engine);
        child
            .args([
                "-m",
                model.to_str().unwrap(),
                "-c",
                "4096",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .env("FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN", lifetime.token())
            .env(BIND_DIAGNOSTIC_ENV, &bind_diagnostic)
            .env(READY_MARKER_ENV, &ready_marker)
            .current_dir(workspace)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        test_process_containment::configure_command_parent_death_signal(&mut child)
            .expect("configure direct fixture parent-death containment");
        configure_fixture_owner_environment(&mut child);
        let mut child = child
            .spawn()
            .map(ChildGuard::new)
            .unwrap_or_else(|error| panic!("could not spawn {label}: {error}"));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let exit = match child.child_mut().try_wait() {
                Ok(exit) => exit,
                Err(observe_error) => {
                    let lifetime_cleanup = lifetime.finish();
                    let child_cleanup = child.terminate_and_reap();
                    panic!(
                        "could not observe {label}: {observe_error}; lifetime cleanup={lifetime_cleanup:?}; child cleanup={child_cleanup:?}"
                    );
                }
            };
            if let Some(status) = exit {
                let address_in_use = marker_matches(&bind_diagnostic, ADDRESS_IN_USE_DIAGNOSTIC);
                child.disarm_reaped().unwrap_or_else(|error| {
                    panic!("could not confirm exited {label} was reaped: {error}")
                });
                lifetime.finish().unwrap_or_else(|error| {
                    panic!("could not clean exited {label} lifetime state: {error}")
                });
                remove_marker(&bind_diagnostic);
                remove_marker(&ready_marker);
                if address_in_use && attempt < PORT_ATTEMPTS {
                    break;
                }
                if address_in_use {
                    panic!("{label} exhausted {PORT_ATTEMPTS} diagnosed address-in-use attempts");
                }
                panic!("{label} exited before readiness without a diagnosed bind race: {status}");
            }

            if marker_matches(&ready_marker, READY_MARKER) {
                assert!(
                    !bind_diagnostic.exists(),
                    "{label} reported both bind failure and readiness"
                );
                if endpoint_is_healthy(port) {
                    remove_marker(&ready_marker);
                    return RunningFixture {
                        port,
                        lifetime,
                        child,
                    };
                }
            }

            if Instant::now() >= deadline {
                let lifetime_cleanup = lifetime.finish();
                let child_cleanup = child.terminate_and_reap();
                remove_marker(&bind_diagnostic);
                remove_marker(&ready_marker);
                panic!(
                    "{label} exceeded the bounded readiness watchdog; lifetime cleanup={lifetime_cleanup:?}; child cleanup={child_cleanup:?}"
                );
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    unreachable!("the diagnosed bind retry loop always returns or panics")
}

struct TailscaleLifecycleEvidence {
    requests: Vec<LocalApiRequestLog>,
    initial_state: serde_json::Value,
    final_state: serde_json::Value,
    token: String,
    mount_path: String,
    proxy_target: String,
    remote_base_url: String,
    up_stdout: String,
    status_stdout: String,
    down_stdout: String,
}

#[cfg(target_os = "linux")]
#[test]
fn lifecycle_fixture_exits_when_exact_owner_pidfd_signals() {
    if std::env::var_os(OWNER_DEATH_PROBE_MODE_ENV).is_some() {
        run_owner_death_probe().expect("run sacrificial lifecycle-fixture owner probe");
        return;
    }

    let _lifecycle_lock = lifecycle_test_lock();
    let root = tempfile::tempdir().expect("create owner-death regression directory");
    let model = root.path().join("owner-death-model.gguf");
    let lifetime_token = root.path().join("owner-death.lifetime");
    fs::write(&model, b"model-free owner-death fixture").expect("write owner-death fixture model");
    fs::write(
        &lifetime_token,
        b"owner death, not token removal, must stop this fixture",
    )
    .expect("write owner-death lifetime token");

    let mut launched = None;
    for attempt in 1..=PORT_ATTEMPTS {
        let port = unused_port();
        let ready = root.path().join(format!("owner-death-{attempt}.ready"));
        let bind_diagnostic = root.path().join(format!("owner-death-{attempt}.bind"));
        let stdout_path = root.path().join(format!("owner-death-{attempt}.stdout"));
        let stderr_path = root.path().join(format!("owner-death-{attempt}.stderr"));
        let stdout = File::create(&stdout_path).expect("create sacrificial-owner stdout capture");
        let stderr = File::create(&stderr_path).expect("create sacrificial-owner stderr capture");
        let mut command = Command::new(
            std::env::current_exe().expect("resolve Cargo-provided lifecycle test harness"),
        );
        command
            .args([
                "--exact",
                OWNER_DEATH_PROBE_TEST_NAME,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(OWNER_DEATH_PROBE_MODE_ENV, "owner")
            .env(OWNER_DEATH_PROBE_FIXTURE_ENV, fixture_executable())
            .env(OWNER_DEATH_PROBE_MODEL_ENV, &model)
            .env(OWNER_DEATH_PROBE_TOKEN_ENV, &lifetime_token)
            .env(OWNER_DEATH_PROBE_READY_ENV, &ready)
            .env(OWNER_DEATH_PROBE_PORT_ENV, port.to_string())
            .env(BIND_DIAGNOSTIC_ENV, &bind_diagnostic)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        let mut owner = test_process_containment::ContainedChild::spawn(&mut command)
            .expect("spawn source-defined sacrificial owner");
        let spawned_owner_pid = owner.child().id();
        match wait_for_owner_death_coordinate(&mut owner, &ready) {
            Ok(coordinate) => {
                if coordinate.owner_pid != spawned_owner_pid || coordinate.port != port {
                    let owner_cleanup = owner.terminate_and_reap();
                    let listener_closed = wait_until(|| endpoint_is_closed(port));
                    panic!(
                        "atomic owner-death coordinate did not identify the spawned owner and requested listener: spawned_owner={spawned_owner_pid}, published_owner={}, requested_port={port}, published_port={}; owner cleanup={owner_cleanup:?}; listener closed={listener_closed}",
                        coordinate.owner_pid, coordinate.port
                    );
                }
                if bind_diagnostic.exists() {
                    let owner_cleanup = owner.terminate_and_reap();
                    let listener_closed = wait_until(|| endpoint_is_closed(port));
                    panic!(
                        "owner-death fixture published both readiness and a bind failure; owner cleanup={owner_cleanup:?}; listener closed={listener_closed}"
                    );
                }
                launched = Some((owner, coordinate, ready, stdout_path, stderr_path));
                break;
            }
            Err(error) => {
                let address_in_use = marker_matches(&bind_diagnostic, ADDRESS_IN_USE_DIAGNOSTIC);
                let owner_cleanup = owner.terminate_and_reap();
                let stdout = fs::read(&stdout_path).unwrap_or_default();
                let stderr = fs::read(&stderr_path).unwrap_or_default();
                remove_marker(&bind_diagnostic);
                remove_marker(&ready);
                if address_in_use && attempt < PORT_ATTEMPTS {
                    continue;
                }
                let listener_closed = wait_until(|| endpoint_is_closed(port));
                panic!(
                    "source-defined sacrificial owner did not become ready: {error}; owner cleanup={owner_cleanup:?}; listener closed={listener_closed}\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
            }
        }
    }

    let (mut owner, coordinate, ready, stdout_path, stderr_path) =
        launched.expect("diagnosed owner-death bind retry loop must launch or panic");
    let mut exact_owner = match ExternalProcessGuard::observe_child(
        coordinate.owner_pid,
        &coordinate.owner_start_token,
    ) {
        Ok(process) => process,
        Err(error) => {
            let owner_cleanup = owner.terminate_and_reap();
            let listener_closed = wait_until(|| endpoint_is_closed(coordinate.port));
            panic!(
                "could not retain the exact sacrificial owner pidfd: {error}; owner cleanup={owner_cleanup:?}; listener closed={listener_closed}"
            );
        }
    };
    let mut exact_fixture = match ExternalProcessGuard::acquire(
        coordinate.fixture_pid,
        &coordinate.fixture_start_token,
    ) {
        Ok(process) => process,
        Err(error) => {
            let owner_cleanup = exact_owner.terminate_and_disarm();
            let owner_reap = owner.terminate_and_reap();
            let listener_closed = wait_until(|| endpoint_is_closed(coordinate.port));
            panic!(
                "could not retain the exact lifecycle-fixture pidfd: {error}; exact owner cleanup={owner_cleanup:?}; owner reap={owner_reap:?}; listener closed={listener_closed}"
            );
        }
    };

    assert!(
        exact_owner.running(),
        "sacrificial owner exited before the exact-owner-death assertion"
    );
    assert!(
        exact_fixture.running(),
        "lifecycle fixture exited before its exact owner was terminated"
    );
    assert!(
        endpoint_is_healthy(coordinate.port),
        "lifecycle fixture was not healthy at its atomically published coordinate"
    );
    assert!(
        lifetime_token.is_file(),
        "fixture lifetime token disappeared before owner termination"
    );

    exact_owner
        .terminate_and_disarm()
        .expect("terminate only the retained exact sacrificial-owner pidfd");
    if !wait_until(|| !exact_fixture.running()) {
        let fixture_cleanup = exact_fixture.terminate_and_disarm();
        panic!(
            "lifecycle fixture did not exit when its retained exact-owner pidfd signalled owner death; fixture cleanup={fixture_cleanup:?}"
        );
    }
    exact_fixture
        .disarm_after_exit()
        .expect("confirm exact lifecycle-fixture exit and adopted reaping after owner death");
    owner
        .wait_for_exit_and_disarm(CHILD_EXIT_GRACE)
        .expect("reap sacrificial owner and drain its source-owned scope");
    assert!(
        wait_until(|| endpoint_is_closed(coordinate.port)),
        "lifecycle fixture listener remained open after exact owner death"
    );
    assert!(
        lifetime_token.is_file(),
        "lifetime-token removal, rather than exact owner death, could have caused fixture exit"
    );

    fs::remove_file(&lifetime_token).expect("remove completed owner-death lifetime token");
    remove_marker(&ready);
    let stdout = fs::read(stdout_path).expect("read sacrificial-owner stdout capture");
    let stderr = fs::read(stderr_path).expect("read sacrificial-owner stderr capture");
    assert!(
        stdout.is_empty() || String::from_utf8_lossy(&stdout).contains("running 1 test"),
        "sacrificial owner emitted unexpected stdout: {}",
        String::from_utf8_lossy(&stdout)
    );
    assert!(
        stderr.is_empty(),
        "sacrificial owner emitted unexpected stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
}

fn run_tailscale_lifecycle_fixture() -> TailscaleLifecycleEvidence {
    let _lifecycle_lock = lifecycle_test_lock();
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let appdata = root.path().join("isolated-config");
    let local_dir = workspace.join(".ferric");
    let global_dir = appdata.join("ferric");
    write_sentinel(&local_dir, "workspace");
    write_sentinel(&global_dir, "global");
    fs::write(root.path().join(SENTINEL_NAME), "root").unwrap();

    let (bin_dir, engine) = install_fixture(root.path());
    assert!(engine.is_file());
    let model = root.path().join("dummy-model.gguf");
    fs::write(&model, b"model-free fixture").unwrap();
    let initial_state = initial_tailscale_state();
    let local = local_dir.join("server.json");
    let global = global_dir.join("server.json");
    let localapi = FakeLocalApi::start(initial_state.clone(), local.clone(), global.clone());

    let doctor_port = unused_port();
    let doctor = run_cli_output_bounded(
        "real `ferric server doctor --tailscale`",
        isolated_localapi_ferric(&workspace, &appdata, &bin_dir, localapi.address()).args([
            "server",
            "doctor",
            "--tailscale",
            "--model",
            model.to_str().unwrap(),
            "--ctx",
            "4096",
            "--port",
            &doctor_port.to_string(),
        ]),
    );
    assert_success("real `ferric server doctor --tailscale`", doctor);
    assert_eq!(localapi.serve_config(), initial_state);

    let (port, mut process_lifetime, up) = launch_tailscale_managed_fixture_with_retry(
        root.path(),
        &workspace,
        &appdata,
        &bin_dir,
        &model,
        localapi.address(),
    );
    let lifetime_token = process_lifetime.token().to_path_buf();
    let up_stdout = String::from_utf8(up.stdout.clone()).unwrap();
    assert_success("real `ferric server up --tailscale`", up);

    let raw = fs::read(&local).unwrap();
    assert_eq!(fs::read(&global).unwrap(), raw);
    let runfile: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let pid = runfile["pid"].as_u64().unwrap() as u32;
    let start_token = runfile["process_identity"]["start_token"].as_str().unwrap();
    let token = runfile["tailscale_serve"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    let mount_path = runfile["tailscale_serve"]["mount_path"]
        .as_str()
        .unwrap()
        .to_string();
    let proxy_target = runfile["tailscale_serve"]["proxy_target"]
        .as_str()
        .unwrap()
        .to_string();
    let remote_base_url = runfile["tailscale_serve"]["remote_base_url"]
        .as_str()
        .unwrap()
        .to_string();
    let local_base_url = format!("http://127.0.0.1:{port}/v1");
    assert_eq!(runfile["schema_version"], 2);
    assert_eq!(runfile["tailscale"], true);
    assert_eq!(runfile["tailscale_serve"]["apply_confirmed"], true);
    assert_eq!(runfile["base_url"], local_base_url);
    assert_eq!(runfile["port"], port);
    assert_eq!(token.len(), 32);
    assert!(
        token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert_eq!(mount_path, format!("/_ferric/{token}"));
    assert_eq!(proxy_target, format!("http://127.0.0.1:{port}"));
    assert_eq!(
        remote_base_url,
        format!("https://{TAILSCALE_FQDN}{mount_path}/v1")
    );
    assert!(up_stdout.contains(&format!("server ready: {local_base_url} (pid {pid})")));
    assert!(up_stdout.contains(&format!(
        "Tailscale Serve endpoint ready: {remote_base_url}"
    )));
    assert!(up_stdout.contains(&format!("registered locally at {}", local.display())));
    assert!(up_stdout.contains(&format!("registered globally at {}", global.display())));

    let active_state = localapi.serve_config();
    assert_eq!(
        active_state["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"][UNRELATED_SERVE_PATH],
        initial_state["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"][UNRELATED_SERVE_PATH]
    );
    assert_eq!(
        active_state["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"][&mount_path]["Proxy"],
        proxy_target
    );

    let mut fixture_guard = ExternalProcessGuard::acquire(pid, start_token)
        .expect("retain exact Tailscale up-launched fixture process object");
    assert!(endpoint_is_healthy(port));

    let status = run_cli_output_bounded(
        "real `ferric server status` for Tailscale",
        isolated_localapi_ferric(&workspace, &appdata, &bin_dir, localapi.address())
            .args(["server", "status"]),
    );
    let status_stdout = String::from_utf8(status.stdout.clone()).unwrap();
    assert_success("real `ferric server status` for Tailscale", status);
    assert!(status_stdout.contains(&local_base_url));
    assert!(status_stdout.contains(&remote_base_url));
    assert!(status_stdout.contains("[tailscale] active"));

    let down = run_cli_output_bounded(
        "real `ferric server down` for Tailscale",
        isolated_localapi_ferric(&workspace, &appdata, &bin_dir, localapi.address())
            .args(["server", "down"]),
    );
    let down_stdout = String::from_utf8(down.stdout.clone()).unwrap();
    assert_success("real `ferric server down` for Tailscale", down);
    assert!(down_stdout.contains("[removed] local registration"));
    assert!(down_stdout.contains("[removed] global registration"));
    assert!(down_stdout.contains("[state] stopped managed server"));

    if !wait_until(|| !fixture_guard.running()) {
        let process_cleanup = fixture_guard.terminate_and_disarm();
        let lifetime_cleanup = process_lifetime.finish();
        panic!(
            "Tailscale up-launched fixture PID {pid} remained alive after down; process cleanup={process_cleanup:?}; lifetime cleanup={lifetime_cleanup:?}"
        );
    }
    fixture_guard
        .disarm_after_exit()
        .expect("confirm exact Tailscale fixture process exit before disarming cleanup");
    process_lifetime
        .finish()
        .expect("clean Tailscale fixture lifetime state");
    assert!(wait_until(|| endpoint_is_closed(port)));
    assert!(!lifetime_token.exists());
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
    assert_eq!(
        fs::read_to_string(root.path().join(SENTINEL_NAME)).unwrap(),
        "root"
    );

    let state_after_first_down = localapi.serve_config();
    assert_eq!(state_after_first_down, initial_state);
    let requests_after_first_down = localapi.requests();

    let repeated_down = run_cli_output_bounded(
        "repeated real `ferric server down` for Tailscale",
        isolated_localapi_ferric(&workspace, &appdata, &bin_dir, localapi.address())
            .args(["server", "down"]),
    );
    let repeated_down_stdout = String::from_utf8(repeated_down.stdout.clone()).unwrap();
    assert_success(
        "repeated real `ferric server down` for Tailscale",
        repeated_down,
    );
    assert!(repeated_down_stdout.contains("[state] no server registered"));
    assert!(endpoint_is_closed(port));
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
    let final_state = localapi.serve_config();
    assert_eq!(final_state, initial_state);
    let requests = localapi.requests();
    assert_eq!(
        requests.len(),
        requests_after_first_down.len(),
        "repeated down after successful cleanup must not invoke Tailscale"
    );
    localapi.stop();

    TailscaleLifecycleEvidence {
        requests,
        initial_state,
        final_state,
        token,
        mount_path,
        proxy_target,
        remote_base_url,
        up_stdout,
        status_stdout,
        down_stdout,
    }
}

#[test]
fn model_free_server_lifecycle_fixture_e2e() {
    let _lifecycle_lock = lifecycle_test_lock();
    let root = tempfile::tempdir().unwrap();
    let workspace_a = root.path().join("workspace-a");
    let workspace_b = root.path().join("workspace-b");
    let appdata = root.path().join("isolated-config");
    let local_a_dir = workspace_a.join(".ferric");
    let local_b_dir = workspace_b.join(".ferric");
    let global_dir = appdata.join("ferric");
    write_sentinel(&local_a_dir, "workspace-a");
    write_sentinel(&local_b_dir, "workspace-b");
    write_sentinel(&global_dir, "global");
    fs::write(root.path().join(SENTINEL_NAME), "root").unwrap();

    let (bin_dir, _engine) = install_fixture(root.path());
    let model = root.path().join("dummy-model.gguf");
    fs::write(&model, b"model-free fixture").unwrap();
    let baseline = snapshot_tree(root.path());

    // The token and its independent watchdog exist before the blocking CLI
    // launch. A bind retry is allowed only when the fixture writes its exact,
    // private address-in-use diagnostic.
    let (port, mut process_lifetime) =
        launch_managed_fixture_with_retry(root.path(), &workspace_b, &appdata, &bin_dir, &model);
    let lifetime_token = process_lifetime.token().to_path_buf();

    let local_a = local_a_dir.join("server.json");
    let local_b = local_b_dir.join("server.json");
    let global = global_dir.join("server.json");
    let live_raw = fs::read(&global).unwrap();
    assert_eq!(
        fs::read(&local_b).unwrap(),
        live_raw,
        "up must publish byte-identical local/global mirrors"
    );
    let live: serde_json::Value = serde_json::from_slice(&live_raw).unwrap();
    let pid = live["pid"].as_u64().unwrap() as u32;
    let start_token = live["process_identity"]["start_token"]
        .as_str()
        .expect("published fixture start token");
    let mut fixture_guard = ExternalProcessGuard::acquire(pid, start_token)
        .expect("retain exact up-launched fixture process object");
    assert!(endpoint_is_healthy(port));

    // One incomplete client must not serialize the listener. The fixture's
    // per-connection worker leaves the accept loop free for a second health
    // request while this socket deliberately sends no HTTP bytes.
    let address = format!("127.0.0.1:{port}").parse().unwrap();
    let slow_connection = TcpStream::connect_timeout(&address, Duration::from_millis(250))
        .expect("open deliberately incomplete fixture connection");
    thread::sleep(Duration::from_millis(100));
    assert!(
        endpoint_is_healthy(port),
        "an incomplete HTTP client blocked independent fixture health handling"
    );
    drop(slow_connection);

    assert_success(
        "status from originating workspace B",
        run_cli_output_bounded(
            "status from originating workspace B",
            isolated_ferric(&workspace_b, &appdata, &bin_dir).args(["server", "status"]),
        ),
    );

    // A stale current-workspace record names this test runner with a creation
    // token that cannot match it. Its same-port listener owner is the verified
    // global/origin process, so lossless resolution must select B, not shadow
    // it or signal this test process.
    let mut stale = live.clone();
    stale["pid"] = serde_json::json!(std::process::id());
    let old_token = stale["process_identity"]["start_token"]
        .as_str()
        .unwrap()
        .to_string();
    let alternative = canonical_test_start_token(1);
    stale["process_identity"]["start_token"] = serde_json::json!(if old_token == alternative {
        canonical_test_start_token(2)
    } else {
        alternative
    });
    stale["origin_local_runfile"] = serde_json::json!(local_a.to_string_lossy().into_owned());
    write_bytes(&local_a, &serde_json::to_vec_pretty(&stale).unwrap());

    assert_success(
        "status from stale workspace A",
        run_cli_output_bounded(
            "status from stale workspace A",
            isolated_ferric(&workspace_a, &appdata, &bin_dir).args(["server", "status"]),
        ),
    );
    assert_success(
        "down from stale workspace A",
        run_cli_output_bounded(
            "down from stale workspace A",
            isolated_ferric(&workspace_a, &appdata, &bin_dir).args(["server", "down"]),
        ),
    );

    if !wait_until(|| !fixture_guard.running()) {
        let process_cleanup = fixture_guard.terminate_and_disarm();
        let lifetime_cleanup = process_lifetime.finish();
        panic!(
            "up-launched fixture PID {pid} remained alive after down; process cleanup={process_cleanup:?}; lifetime cleanup={lifetime_cleanup:?}"
        );
    }
    fixture_guard
        .disarm_after_exit()
        .expect("confirm exact managed fixture process exit before disarming cleanup");
    if !wait_until(|| !endpoint_is_healthy(port)) {
        let lifetime_cleanup = process_lifetime.finish();
        panic!(
            "fixture listener 127.0.0.1:{port} remained healthy after down; lifetime cleanup={lifetime_cleanup:?}"
        );
    }
    process_lifetime
        .finish()
        .expect("clean managed fixture lifetime state");
    assert!(
        wait_until(|| endpoint_is_closed(port)),
        "fixture listener 127.0.0.1:{port} remained open after exact process exit"
    );
    assert!(
        !lifetime_token.exists(),
        "fixture lifetime coordination token remained after teardown"
    );
    assert_only_sentinel(&local_a_dir, "workspace-a");
    assert_only_sentinel(&local_b_dir, "workspace-b");
    assert_only_sentinel(&global_dir, "global");
    assert_eq!(
        fs::read_to_string(root.path().join(SENTINEL_NAME)).unwrap(),
        "root"
    );
    assert_eq!(
        snapshot_tree(root.path()),
        baseline,
        "the complete fixture tree changed: an owned registration, stage, coordination artifact, or unrelated mutation remained"
    );
}

#[test]
fn tailscale_localapi_lifecycle_preserves_unrelated_state() {
    let evidence = run_tailscale_lifecycle_fixture();
    assert_closed_localapi_log(&evidence);
    assert_eq!(evidence.final_state, evidence.initial_state);
    assert_eq!(
        evidence.final_state["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"]
            [UNRELATED_SERVE_PATH],
        serde_json::json!({"Text": "unrelated handler must survive"})
    );
    assert_eq!(
        evidence.final_state["Services"]["svc:demo"]["TCP"]["9000"]["TCPForward"],
        "127.0.0.1:9"
    );
    assert_eq!(evidence.token.len(), 32);
    assert!(
        evidence
            .token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert_eq!(evidence.mount_path, format!("/_ferric/{}", evidence.token));
    assert_eq!(
        evidence.remote_base_url,
        format!("https://{TAILSCALE_FQDN}{}/v1", evidence.mount_path)
    );
    assert!(evidence.up_stdout.contains(&evidence.proxy_target));
    assert!(evidence.up_stdout.contains(&evidence.remote_base_url));
    assert!(evidence.status_stdout.contains(&evidence.remote_base_url));
    assert!(evidence.status_stdout.contains("[tailscale] active"));
    assert!(
        evidence
            .down_stdout
            .contains("[state] stopped managed server")
    );
}

fn assert_closed_localapi_log(evidence: &TailscaleLifecycleEvidence) {
    const STATUS_PATH: &str = "/localapi/v0/status?peers=false";
    const CONFIG_PATH: &str = "/localapi/v0/serve-config";
    assert!(!evidence.requests.is_empty());
    for (index, request) in evidence.requests.iter().enumerate() {
        let mut expected_headers = vec![
            ("Host".to_string(), "local-tailscaled.sock".to_string()),
            ("Tailscale-Cap".to_string(), "142".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
            ("Connection".to_string(), "keep-alive".to_string()),
        ];
        if request.method == "POST" {
            expected_headers.extend([
                (
                    "If-Match".to_string(),
                    request
                        .if_match
                        .clone()
                        .expect("POST request must carry If-Match"),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Content-Length".to_string(), request.body.len().to_string()),
            ]);
        }
        assert_eq!(
            request.headers, expected_headers,
            "LocalAPI request headers changed for {} {}",
            request.method, request.path
        );
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", STATUS_PATH) => {
                assert!(request.if_match.is_none());
                assert!(request.body.is_empty());
            }
            ("GET", CONFIG_PATH) => {
                assert!(request.if_match.is_none());
                assert!(request.body.is_empty());
                let before = index
                    .checked_sub(1)
                    .and_then(|index| evidence.requests.get(index));
                let after = evidence.requests.get(index + 1);
                assert!(
                    before.is_some_and(|neighbor| {
                        neighbor.connection == request.connection
                            && neighbor.method == "GET"
                            && neighbor.path == STATUS_PATH
                    }),
                    "Serve-config GET was not preceded by same-session status: {request:?}"
                );
                assert!(
                    after.is_some_and(|neighbor| {
                        neighbor.connection == request.connection
                            && neighbor.method == "GET"
                            && neighbor.path == STATUS_PATH
                    }),
                    "Serve-config GET was not followed by same-session status: {request:?}"
                );
            }
            ("POST", CONFIG_PATH) => {
                let window = evidence
                    .requests
                    .get(index.saturating_sub(3)..index + 4)
                    .expect("POST must have a complete seven-request transaction window");
                assert!(
                    window
                        .iter()
                        .all(|neighbor| neighbor.connection == request.connection),
                    "POST transaction crossed LocalAPI connections: {window:?}"
                );
                assert_eq!(
                    window
                        .iter()
                        .map(|neighbor| (neighbor.method.as_str(), neighbor.path.as_str()))
                        .collect::<Vec<_>>(),
                    vec![
                        ("GET", STATUS_PATH),
                        ("GET", CONFIG_PATH),
                        ("GET", STATUS_PATH),
                        ("POST", CONFIG_PATH),
                        ("GET", STATUS_PATH),
                        ("GET", CONFIG_PATH),
                        ("GET", STATUS_PATH),
                    ],
                    "POST was not enclosed by exact same-session identity/config sandwiches"
                );
            }
            _ => panic!(
                "fixture recorded forbidden or broad LocalAPI request: {} {}",
                request.method, request.path
            ),
        }
    }

    let posts = evidence
        .requests
        .iter()
        .filter(|request| request.method == "POST")
        .collect::<Vec<_>>();
    assert_eq!(
        posts.len(),
        2,
        "lifecycle must perform exactly one apply CAS and one cleanup CAS"
    );
    assert!(
        posts.iter().all(|request| request
            .journal_on_post
            .as_ref()
            .is_some_and(|journal| journal.mirrors_equal)),
        "both ownership journals must exist and match before every CAS"
    );

    for (post_index, post) in posts.iter().enumerate() {
        let journal = post
            .journal_on_post
            .as_ref()
            .expect("POST must capture typed journal state");
        assert_eq!(journal.schema_version, Some(2));
        assert_eq!(journal.tailscale, Some(true));
        assert_eq!(journal.ownership_version, Some(2));
        assert_eq!(
            journal.stable_node_id.as_deref(),
            Some(TAILSCALE_STABLE_NODE_ID)
        );
        assert_eq!(journal.fqdn.as_deref(), Some(TAILSCALE_FQDN));
        assert_eq!(journal.https_port, Some(443));
        assert_eq!(
            journal.mount_path.as_deref(),
            Some(evidence.mount_path.as_str())
        );
        assert_eq!(
            journal.proxy_target.as_deref(),
            Some(evidence.proxy_target.as_str())
        );
        assert_eq!(
            journal.remote_base_url.as_deref(),
            Some(evidence.remote_base_url.as_str())
        );
        let before_hash = journal
            .before_status_sha256
            .as_deref()
            .expect("typed ownership must include the pre-state digest");
        assert_eq!(before_hash.len(), 64);
        assert!(
            before_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert_eq!(journal.tcp_map_preexisting, Some(true));
        assert_eq!(journal.tcp_https_preexisting, Some(true));
        assert_eq!(journal.web_map_preexisting, Some(true));
        assert_eq!(journal.web_host_preexisting, Some(true));
        assert_eq!(journal.apply_confirmed, Some(post_index == 1));
        assert!(
            journal.target_healthy,
            "proxy target was not HTTP-healthy at {} CAS",
            if post_index == 0 { "apply" } else { "cleanup" }
        );
    }

    let initial_raw = serde_json::to_vec(&evidence.initial_state).unwrap();
    let initial_etag = hex::encode(Sha256::digest(&initial_raw));
    assert_eq!(posts[0].if_match.as_deref(), Some(initial_etag.as_str()));
    let applied: serde_json::Value = serde_json::from_slice(&posts[0].body).unwrap();
    let applied_raw = serde_json::to_vec(&applied).unwrap();
    let applied_etag = hex::encode(Sha256::digest(&applied_raw));
    assert_eq!(posts[1].if_match.as_deref(), Some(applied_etag.as_str()));

    assert_eq!(
        applied["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"][UNRELATED_SERVE_PATH],
        evidence.initial_state["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"]
            [UNRELATED_SERVE_PATH]
    );
    assert_eq!(
        applied["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"][&evidence.mount_path]["Proxy"],
        evidence.proxy_target
    );
    let cleaned: serde_json::Value = serde_json::from_slice(&posts[1].body).unwrap();
    assert_eq!(cleaned, evidence.initial_state);
}

#[test]
fn tailscale_localapi_log_contains_no_broad_mutation_or_retry() {
    let evidence = run_tailscale_lifecycle_fixture();
    assert_closed_localapi_log(&evidence);
}

#[test]
fn ordinary_ferric_ignores_lifecycle_localapi_override() {
    let _lifecycle_lock = lifecycle_test_lock();
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let appdata = root.path().join("isolated-config");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&appdata).unwrap();
    let (bin_dir, _engine) = install_fixture(root.path());
    let model = root.path().join("dummy-model.gguf");
    fs::write(&model, b"model-free fixture").unwrap();
    let localapi = FakeLocalApi::start(
        initial_tailscale_state(),
        workspace.join(".ferric/server.json"),
        appdata.join("ferric/server.json"),
    );
    let mut command = isolated_production_ferric(&workspace, &appdata, &bin_dir);
    command
        .env(
            TAILSCALE_LOCALAPI_TEST_TCP_ENV,
            localapi.address().to_string(),
        )
        .args([
            "server",
            "doctor",
            "--tailscale",
            "--model",
            model.to_str().unwrap(),
            "--ctx",
            "4096",
            "--port",
            &unused_port().to_string(),
        ]);
    let _ = run_cli_output_bounded("ordinary ferric override isolation", &mut command);
    assert!(
        localapi.requests().is_empty(),
        "ordinary ferric binary honored the lifecycle-only LocalAPI TCP override"
    );
    localapi.stop();
}

#[test]
fn legacy_adoption_then_down_cli_e2e() {
    let _lifecycle_lock = lifecycle_test_lock();
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let appdata = root.path().join("isolated-config");
    let local_dir = workspace.join(".ferric");
    let global_dir = appdata.join("ferric");
    write_sentinel(&local_dir, "workspace");
    write_sentinel(&global_dir, "global");
    let (bin_dir, engine) = install_fixture(root.path());
    let model = root.path().join("dummy-model.gguf");
    fs::write(&model, b"model-free fixture").unwrap();
    let RunningFixture {
        port,
        lifetime: mut process_lifetime,
        child: mut fixture,
    } = launch_direct_fixture_with_retry(
        root.path(),
        &workspace,
        &engine,
        &model,
        "legacy-adoption-fixture",
    );
    let process_lifetime_token = process_lifetime.token().to_path_buf();
    let pid = fixture.child_mut().id();

    let local = local_dir.join("server.json");
    let global = global_dir.join("server.json");
    let legacy = serde_json::json!({
        "schema_version": 1,
        "engine": "llama-server",
        "pid": pid,
        "port": port,
        "base_url": format!("http://127.0.0.1:{port}/v1"),
        "tailscale": false,
        "model": model.to_string_lossy().into_owned(),
        "context_size": 4096
    });
    let legacy_raw = serde_json::to_vec_pretty(&legacy).unwrap();
    write_bytes(&local, &legacy_raw);
    write_bytes(&global, &legacy_raw);

    let legacy_reason =
        format!("live schema-1 PID {pid} has no creation identity and cannot authorize teardown");
    let expected_diagnostics = vec![
        format!(
            "[diagnostic] local registration {}: {legacy_reason}",
            local.display()
        ),
        format!(
            "[diagnostic] global registration {}: {legacy_reason}",
            global.display()
        ),
    ];
    let expected_adopt_command = format!("ferric server adopt --pid {pid}");

    let status_before_adoption = run_cli_output_bounded(
        "status before legacy adoption",
        isolated_ferric(&workspace, &appdata, &bin_dir)
            .env(
                "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN",
                &process_lifetime_token,
            )
            .args(["server", "status"]),
    );
    assert!(
        !status_before_adoption.status.success(),
        "status unexpectedly accepted a live schema-1 registration:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&status_before_adoption.stdout),
        String::from_utf8_lossy(&status_before_adoption.stderr)
    );
    let status_stdout = output_lines(
        "status before legacy adoption",
        "stdout",
        &status_before_adoption.stdout,
    );
    assert_eq!(
        status_stdout.len(),
        4,
        "status must render both registrations, state, and one complete next action"
    );
    assert!(status_stdout[0].starts_with(&format!(
        "[captured] local registration {}:",
        local.display()
    )));
    assert!(status_stdout[1].starts_with(&format!(
        "[captured] global registration {}:",
        global.display()
    )));
    assert_eq!(status_stdout[2], "[state] unverifiable");
    assert_eq!(
        status_stdout[3],
        format!(
            "[next] verify and record the live legacy process without signalling it: `{expected_adopt_command}`"
        )
    );
    assert_eq!(
        output_lines(
            "status before legacy adoption",
            "stderr",
            &status_before_adoption.stderr,
        ),
        expected_diagnostics
    );
    assert!(
        fixture.child_mut().try_wait().unwrap().is_none(),
        "status must not signal the live schema-1 fixture"
    );
    assert!(endpoint_is_healthy(port));
    assert_registration_and_sentinel(&local_dir, "workspace", &legacy_raw);
    assert_registration_and_sentinel(&global_dir, "global", &legacy_raw);

    let down_before_adoption = run_cli_output_bounded(
        "down before legacy adoption",
        isolated_ferric(&workspace, &appdata, &bin_dir)
            .env(
                "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN",
                &process_lifetime_token,
            )
            .args(["server", "down"]),
    );
    assert_failed_output(
        "down before legacy adoption",
        &down_before_adoption,
        &[
            format!(
                "[held] local registration {} detail=typed discovery blocked teardown mutation",
                local.display()
            ),
            format!(
                "[held] global registration {} detail=typed discovery blocked teardown mutation",
                global.display()
            ),
            "[state] teardown blocked; registrations kept".to_string(),
            format!("[next] {expected_adopt_command}"),
        ],
        &expected_diagnostics,
    );
    assert!(
        fixture.child_mut().try_wait().unwrap().is_none(),
        "down must not signal the live schema-1 fixture"
    );
    assert!(endpoint_is_healthy(port));
    assert_registration_and_sentinel(&local_dir, "workspace", &legacy_raw);
    assert_registration_and_sentinel(&global_dir, "global", &legacy_raw);

    assert_success(
        "legacy adoption",
        run_cli_output_bounded(
            "legacy adoption",
            isolated_ferric(&workspace, &appdata, &bin_dir)
                .env(
                    "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN",
                    &process_lifetime_token,
                )
                .args(["server", "adopt", "--pid", &pid.to_string()]),
        ),
    );
    assert!(
        fixture.child_mut().try_wait().unwrap().is_none(),
        "adoption must not signal the live fixture"
    );
    assert!(endpoint_is_healthy(port));

    let adopted_local = fs::read(&local).unwrap();
    let adopted_global = fs::read(&global).unwrap();
    assert_eq!(
        adopted_local, adopted_global,
        "adoption must publish byte-identical v2 aliases"
    );
    let adopted: serde_json::Value = serde_json::from_slice(&adopted_local).unwrap();
    assert_eq!(adopted["schema_version"], 2);
    assert_eq!(adopted["pid"], pid);
    assert!(adopted["process_identity"].is_object());
    assert_eq!(
        PathBuf::from(adopted["origin_local_runfile"].as_str().unwrap()),
        local
    );
    assert_success(
        "status after adoption",
        run_cli_output_bounded(
            "status after adoption",
            isolated_ferric(&workspace, &appdata, &bin_dir)
                .env(
                    "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN",
                    &process_lifetime_token,
                )
                .args(["server", "status"]),
        ),
    );
    assert_success(
        "down after adoption",
        run_cli_output_bounded(
            "down after adoption",
            isolated_ferric(&workspace, &appdata, &bin_dir)
                .env(
                    "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN",
                    &process_lifetime_token,
                )
                .args(["server", "down"]),
        ),
    );

    match wait_for_child_exit(fixture.child_mut(), Duration::from_secs(10)) {
        Ok(Some(_)) => {}
        Ok(None) => {
            let cleanup = fixture.terminate_and_reap();
            panic!("adopted fixture remained alive after down; checked cleanup={cleanup:?}");
        }
        Err(wait_error) => {
            let cleanup = fixture.terminate_and_reap();
            panic!(
                "could not wait for adopted fixture after down: {wait_error}; checked cleanup={cleanup:?}"
            );
        }
    }
    // `try_wait` has reaped the exact child. Disarm immediately, before any
    // later assertion can panic and make Drop call Child::kill on a reused PID.
    fixture
        .disarm_reaped()
        .expect("confirm adopted fixture child was reaped before disarming cleanup");
    if !wait_until(|| endpoint_is_closed(port)) {
        let lifetime_cleanup = process_lifetime.finish();
        panic!(
            "adopted fixture listener remained open after down; lifetime cleanup={lifetime_cleanup:?}"
        );
    }
    process_lifetime
        .finish()
        .expect("clean adopted fixture lifetime state");
    assert!(!process_lifetime_token.exists());
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
}
