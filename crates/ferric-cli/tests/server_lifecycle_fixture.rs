//! Black-box lifecycle proofs against the real `ferric` executable.
//!
//! The fixture binary is feature-gated and copied to the exact closed-engine
//! filename, so these tests exercise production CLI parsing, process identity,
//! listener ownership, registration publication, the refusal of retired
//! registrations, and teardown.

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
    Mutex, MutexGuard, OnceLock,
    mpsc::{self, Sender},
};
use std::thread;
use std::time::{Duration, Instant};

use ferric_process::exact::{Access, ExactProcess, ProcessState};

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
const ADDRESS_IN_USE_DIAGNOSTIC: &[u8] = b"ferric-lifecycle-fixture:address-in-use:v1\n";
const READY_MARKER: &[u8] = b"ferric-lifecycle-fixture:ready:v1\n";
const CLI_TIMEOUT: Duration = Duration::from_secs(30);
const CHILD_EXIT_GRACE: Duration = Duration::from_secs(5);
const FIXTURE_LIFETIME_LIMIT: Duration = Duration::from_secs(90);
const PORT_ATTEMPTS: usize = 3;

fn lifecycle_test_lock() -> MutexGuard<'static, ()> {
    ferric_process::testing::contain_current_process()
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
fn linux_process_start_token(pid: u32) -> std::io::Result<String> {
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let boot_id = boot_id.trim();
    if boot_id.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Linux boot ID was empty",
        ));
    }
    let start_ticks = ferric_process::exact::start_ticks(pid)?;
    Ok(format!("linux-boot-id:{boot_id};start-ticks:{start_ticks}"))
}

fn configure_fixture_owner_environment(command: &mut Command) {
    #[cfg(target_os = "linux")]
    command
        .env(OWNER_PID_ENV, std::process::id().to_string())
        .env(
            OWNER_START_TICKS_ENV,
            ferric_process::exact::start_ticks(std::process::id())
                .expect("read lifecycle harness stat")
                .to_string(),
        );

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
    ferric_process::testing::parent_death_signal(&mut command);
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
    ferric_process::abort_on_cleanup_failure(context, error);
}

fn run_cli_output_bounded(label: &str, command: &mut Command) -> Output {
    ferric_process::testing::output(command, CLI_TIMEOUT)
        .unwrap_or_else(|error| panic!("run {label} within owned CLI scope: {error}"))
}

fn run_managed_cli_output_bounded(
    label: &str,
    command: &mut Command,
    lifetime: &mut FixtureLifetimeGuard,
) -> Output {
    use std::io::{Seek, SeekFrom};

    #[cfg(target_os = "linux")]
    let status_directory = tempfile::tempdir().expect("create source CLI handoff coordinate");
    #[cfg(target_os = "linux")]
    let status_path = status_directory.path().join("launcher-status.json");
    #[cfg(target_os = "linux")]
    let mut supervisor = {
        let mut supervisor = Command::new(fixture_executable());
        supervisor
            .arg("--supervise-cli")
            .arg(&status_path)
            .arg(command.get_program())
            .args(command.get_args());
        if let Some(directory) = command.get_current_dir() {
            supervisor.current_dir(directory);
        }
        for (name, value) in command.get_envs() {
            if let Some(value) = value {
                supervisor.env(name, value);
            } else {
                supervisor.env_remove(name);
            }
        }
        supervisor
    };
    #[cfg(target_os = "linux")]
    let command = &mut supervisor;

    let mut stdout = tempfile::tempfile().expect("create managed CLI stdout capture");
    let mut stderr = tempfile::tempfile().expect("create managed CLI stderr capture");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()));
    let child = ferric_process::testing::spawn(command)
        .unwrap_or_else(|error| panic!("spawn {label} in retained lifecycle scope: {error}"));
    // `server up` deliberately hands its server to the test's lifecycle owner.
    // On Linux a live source supervisor anchors this scope and reaps the real
    // launcher before publishing its status. Retaining the exited launcher as
    // the anchor would make production /proc inspection see an unreadable
    // zombie peer. Windows retains the launcher's native Job directly.
    lifetime.scopes.push(child);
    let child = lifetime.scopes.last_mut().unwrap();
    let deadline = Instant::now() + CLI_TIMEOUT;
    #[cfg(windows)]
    let status = loop {
        match child.try_wait_leader() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => panic!("{label} did not complete its bounded launcher handoff: {result:?}"),
        }
    };
    #[cfg(target_os = "linux")]
    let status = loop {
        assert!(
            child
                .try_wait_leader()
                .expect("observe live CLI supervisor")
                .is_none(),
            "{label} source supervisor exited before its managed-server handoff"
        );
        match fs::read(&status_path) {
            Ok(bytes) => {
                use std::os::unix::process::ExitStatusExt;
                let record: serde_json::Value = serde_json::from_slice(&bytes)
                    .expect("decode atomically published launcher status");
                assert_eq!(record["schema_version"], 1);
                assert_eq!(record["supervisor_pid"], child.child().id());
                assert_eq!(record["launcher_reaped"], true);
                let launcher_pid = record["launcher_pid"]
                    .as_u64()
                    .and_then(|pid| u32::try_from(pid).ok())
                    .expect("bounded source launcher PID");
                assert!(launcher_pid > 1 && launcher_pid != child.child().id());
                let raw_status = record["raw_exit_status"]
                    .as_i64()
                    .and_then(|status| i32::try_from(status).ok())
                    .expect("native source launcher exit status");
                assert!(
                    child.try_wait_leader().unwrap().is_none(),
                    "source supervisor lost its live scope anchor"
                );
                break ExitStatus::from_raw(raw_status);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            result => {
                panic!("{label} did not publish a bounded reaped-launcher handoff: {result:?}")
            }
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

/// Cleans an up-launched fixture if a later assertion fails. The exact OS
/// process object is acquired once from the just-published PID and retained;
/// neither liveness checks nor cleanup reopen that numeric PID.
struct ExternalProcessGuard {
    process: ExactProcess,
    armed: bool,
    reap: bool,
}

impl ExternalProcessGuard {
    fn acquire(pid: u32, expected_start_token: &str) -> std::io::Result<Self> {
        // Validate before constructing the terminating guard: a mismatched
        // coordinate never becomes cleanup authority over a replacement PID.
        let process = ExactProcess::open(pid, Access::Terminate)?;
        let exited =
            || Ok::<_, std::io::Error>(process.state(Duration::ZERO)? != ProcessState::Running);
        // Linux start tokens are read by PID. Active checks bracket the read
        // so exit/reuse cannot pair a replacement's facts with this object.
        if exited()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "retained process exited before start-token validation",
            ));
        }
        #[cfg(windows)]
        let observed_start_token = format!("windows-filetime:{}", process.generation()?);
        #[cfg(target_os = "linux")]
        let observed_start_token = linux_process_start_token(pid)?;
        if exited()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "retained process exited during start-token validation",
            ));
        }
        if observed_start_token != expected_start_token {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "published start token {expected_start_token:?} does not match retained process token {observed_start_token:?}"
                ),
            ));
        }
        Ok(Self {
            process,
            armed: true,
            reap: true,
        })
    }

    #[cfg(target_os = "linux")]
    fn observe_child(pid: u32, expected_start_token: &str) -> std::io::Result<Self> {
        let mut guard = Self::acquire(pid, expected_start_token)?;
        // Its owned ProcessTree scope holds the direct leader's reap obligation.
        guard.reap = false;
        Ok(guard)
    }

    fn exited(&self) -> std::io::Result<bool> {
        Ok(self.process.state(Duration::ZERO)? != ProcessState::Running)
    }

    fn running(&self) -> bool {
        // An observation failure is not proof of exit. Keep cleanup and the
        // assertion fail-closed on the retained exact object.
        !self.exited().unwrap_or(false)
    }

    fn disarm_after_exit(&mut self) -> std::io::Result<()> {
        if !self.exited()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "cannot disarm cleanup for a retained process that is still running",
            ));
        }
        // A fixture still owned by its source supervisor may be reaped there;
        // reaping proves that case too.
        if self.reap {
            self.process.reap(CHILD_EXIT_GRACE)?;
        }
        self.armed = false;
        Ok(())
    }

    fn terminate_and_disarm(&mut self) -> std::io::Result<()> {
        self.process.terminate()?;
        if self.process.state(Duration::from_secs(5))? == ProcessState::Running {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out waiting for retained process termination",
            ));
        }
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
    scopes: Vec<ferric_process::ProcessTree>,
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
    owner: &mut ferric_process::ProcessTree,
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
        ferric_process::testing::parent_death_signal(&mut child);
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
        let mut command = ferric_process::testing::harness_command(OWNER_DEATH_PROBE_TEST_NAME);
        command
            .env(OWNER_DEATH_PROBE_MODE_ENV, "owner")
            .env(OWNER_DEATH_PROBE_FIXTURE_ENV, fixture_executable())
            .env(OWNER_DEATH_PROBE_MODEL_ENV, &model)
            .env(OWNER_DEATH_PROBE_TOKEN_ENV, &lifetime_token)
            .env(OWNER_DEATH_PROBE_READY_ENV, &ready)
            .env(OWNER_DEATH_PROBE_PORT_ENV, port.to_string())
            .env(BIND_DIAGNOSTIC_ENV, &bind_diagnostic)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        let mut owner = ferric_process::testing::spawn(&mut command)
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
        .wait_for_exit(CHILD_EXIT_GRACE)
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
fn retired_registrations_block_cli_without_signal_or_delete() {
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
        "retired-registration-fixture",
    );
    let process_lifetime_token = process_lifetime.token().to_path_buf();
    let pid = fixture.child_mut().id();
    let local = local_dir.join("server.json");
    let global = global_dir.join("server.json");
    let ferric = |arguments: &[&str]| {
        let mut command = isolated_ferric(&workspace, &appdata, &bin_dir);
        command
            .env(
                "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN",
                &process_lifetime_token,
            )
            .args(arguments);
        run_cli_output_bounded(&format!("ferric {}", arguments.join(" ")), &mut command)
    };

    // A schema-1 record from a Ferric before process identity, and a record
    // from the retired `server up --tailscale`, both naming the live fixture.
    let schema_1 = serde_json::json!({
        "schema_version": 1,
        "engine": "llama-server",
        "pid": pid,
        "port": port,
        "base_url": format!("http://127.0.0.1:{port}/v1"),
        "tailscale": false,
        "model": model.to_string_lossy().into_owned(),
        "context_size": 4096
    });
    let tailscale = serde_json::json!({
        "schema_version": 2,
        "engine": "llama-server",
        "pid": pid,
        "port": port,
        "base_url": format!("http://127.0.0.1:{port}/v1"),
        "tailscale": true,
        "tailscale_serve": {
            "mount_path": "/_ferric/00112233445566778899aabbccddeeff",
            "apply_confirmed": true
        },
        "process_identity": {
            "start_token": canonical_test_start_token(1),
            "executable": engine.to_string_lossy().into_owned(),
            "argv": [engine_filename()]
        },
        "origin_local_runfile": local.to_string_lossy().into_owned()
    });
    let schema_cleanup = "has schema version 1; only schema 2 is managed, so";
    let tailscale_cleanup = "records a Tailscale Serve publication, which Ferric no longer manages; turn that serve off by hand (for example `tailscale serve --https=443 off`), then";
    let next = "[next] repair or make readable the 2 unverifiable registration coordinate(s), then rerun `ferric server status` (no process action is authorized)";

    for (case, record, cleanup) in [
        ("schema-1", schema_1, schema_cleanup),
        ("tailscale", tailscale, tailscale_cleanup),
    ] {
        let raw = serde_json::to_vec_pretty(&record).unwrap();
        write_bytes(&local, &raw);
        write_bytes(&global, &raw);
        let detail = |scope: &str, path: &Path| {
            format!(
                "{scope} registration {}: {cleanup} stop PID {pid} and delete {}",
                path.display(),
                path.display()
            )
        };
        let local_detail = detail("local", &local);
        let global_detail = detail("global", &global);
        let diagnostics = vec![
            format!("[diagnostic] {local_detail}"),
            format!("[diagnostic] {global_detail}"),
        ];
        let held_facts = "recorded-identity=unavailable observed-identity=not-inspected listener=not-inspected health=not-probed";

        assert_failed_output(
            &format!("{case} status"),
            &ferric(&["server", "status"]),
            &[
                format!("[blocked] {local_detail}; {held_facts}"),
                format!("[blocked] {global_detail}; {held_facts}"),
                "[state] unverifiable".to_string(),
                next.to_string(),
            ],
            &diagnostics,
        );
        assert_failed_output(
            &format!("{case} down"),
            &ferric(&["server", "down"]),
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
                next.to_string(),
            ],
            &diagnostics,
        );
        let doctor_lines = [
            vec![
                "[BLOCKED] managed registration state is unverifiable before engine/model probes"
                    .to_string(),
            ],
            diagnostics.clone(),
            vec![next.to_string()],
        ]
        .concat();
        assert_failed_output(
            &format!("{case} doctor"),
            &ferric(&[
                "server",
                "doctor",
                "--model",
                model.to_str().unwrap(),
                "--port",
                &port.to_string(),
            ]),
            &doctor_lines,
            &[],
        );
        assert!(
            fixture.child_mut().try_wait().unwrap().is_none(),
            "{case}: a retired registration must never authorize a signal"
        );
        assert!(endpoint_is_healthy(port), "{case}");
        assert_registration_and_sentinel(&local_dir, "workspace", &raw);
        assert_registration_and_sentinel(&global_dir, "global", &raw);
    }

    // Following the guidance by hand, stop the PID and delete both records;
    // the lifecycle is then empty again.
    fixture
        .terminate_and_reap()
        .expect("stop the fixture named by the retired registrations");
    process_lifetime
        .finish()
        .expect("clean retired-registration fixture lifetime state");
    fs::remove_file(&local).unwrap();
    fs::remove_file(&global).unwrap();
    let down = ferric(&["server", "down"]);
    assert_eq!(
        output_lines("down after manual cleanup", "stdout", &down.stdout),
        vec!["[state] no server registered".to_string()]
    );
    assert_success("down after manual cleanup", down);
    assert!(!process_lifetime_token.exists());
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
}
