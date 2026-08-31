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

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
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

const SENTINEL_NAME: &str = "unrelated-sentinel.txt";
const BIND_DIAGNOSTIC_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_BIND_DIAGNOSTIC";
const READY_MARKER_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_READY_MARKER";
const TAILSCALE_STATE_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_STATE";
const TAILSCALE_LOG_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_LOG";
const TAILSCALE_LOCAL_JOURNAL_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_LOCAL_JOURNAL";
const TAILSCALE_GLOBAL_JOURNAL_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_GLOBAL_JOURNAL";
const TAILSCALE_FQDN: &str = "example-host.tailnet-example.ts.net";
const UNRELATED_SERVE_PATH: &str = "/unrelated-kept";
const ADDRESS_IN_USE_DIAGNOSTIC: &[u8] = b"ferric-lifecycle-fixture:address-in-use:v1\n";
const READY_MARKER: &[u8] = b"ferric-lifecycle-fixture:ready:v1\n";
const CLI_TIMEOUT: Duration = Duration::from_secs(30);
const CHILD_EXIT_GRACE: Duration = Duration::from_secs(5);
const FIXTURE_LIFETIME_LIMIT: Duration = Duration::from_secs(90);
const PORT_ATTEMPTS: usize = 3;

fn lifecycle_test_lock() -> MutexGuard<'static, ()> {
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

fn tailscale_filename() -> &'static str {
    if cfg!(windows) {
        "tailscale.exe"
    } else {
        "tailscale"
    }
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
    command
}

fn isolated_tailscale_ferric(
    workspace: &Path,
    appdata: &Path,
    bin_dir: &Path,
    state: &Path,
    log: &Path,
) -> Command {
    let mut command = isolated_ferric(workspace, appdata, bin_dir);
    command
        .env(TAILSCALE_STATE_ENV, state)
        .env(TAILSCALE_LOG_ENV, log)
        .env(
            TAILSCALE_LOCAL_JOURNAL_ENV,
            workspace.join(".ferric/server.json"),
        )
        .env(
            TAILSCALE_GLOBAL_JOURNAL_ENV,
            appdata.join("ferric/server.json"),
        );
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

fn run_cli_output_bounded(label: &str, command: &mut Command) -> Output {
    // Files, rather than pipes, prevent an incorrectly spawned daemon from
    // keeping `wait_with_output` blocked by inherited writer handles.
    let capture = tempfile::tempdir().expect("create bounded CLI capture directory");
    let stdout_path = capture.path().join("stdout");
    let stderr_path = capture.path().join("stderr");
    let stdout = File::create(&stdout_path).expect("create CLI stdout capture");
    let stderr = File::create(&stderr_path).expect("create CLI stderr capture");
    let mut child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("could not spawn {label}: {error}"));

    let status = match wait_for_child_exit(&mut child, CLI_TIMEOUT)
        .unwrap_or_else(|error| panic!("could not observe {label}: {error}"))
    {
        Some(status) => status,
        None => {
            let kill = child.kill();
            let reaped = wait_for_child_exit(&mut child, CHILD_EXIT_GRACE)
                .unwrap_or_else(|error| panic!("could not reap timed-out {label}: {error}"));
            let stdout = fs::read(&stdout_path).unwrap_or_default();
            let stderr = fs::read(&stderr_path).unwrap_or_default();
            panic!(
                "{label} exceeded the {CLI_TIMEOUT:?} CLI watchdog; kill={kill:?} reaped={reaped:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
        }
    };
    Output {
        status,
        stdout: fs::read(stdout_path).expect("read bounded CLI stdout"),
        stderr: fs::read(stderr_path).expect("read bounded CLI stderr"),
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
        "FutureState": {"preserved": true}
    })
}

fn write_tailscale_state(path: &Path, state: &serde_json::Value) {
    let mut bytes = serde_json::to_vec_pretty(state).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

fn read_tailscale_state(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn read_tailscale_command_log(path: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
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

        pub fn terminate_for_cleanup(&self) {
            unsafe {
                TerminateProcess(self.handle, 1);
                WaitForSingleObject(self.handle, 5_000);
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
            if descriptor.revents & POLLIN != 0 {
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

        pub fn terminate_for_cleanup(&self) {
            unsafe {
                syscall(
                    SYS_PIDFD_SEND_SIGNAL,
                    self.pidfd.as_raw_fd(),
                    SIGKILL,
                    std::ptr::null::<c_void>(),
                    0_u32,
                );
            }
            let _ = self.poll_exit(5_000);
        }
    }
}

/// Cleans an up-launched fixture if a later assertion fails. The exact OS
/// process object is acquired once from the just-published PID and retained;
/// neither liveness checks nor cleanup reopen that numeric PID.
struct ExternalProcessGuard {
    process: native_process::ExactProcess,
    armed: bool,
}

impl ExternalProcessGuard {
    fn acquire(pid: u32, expected_start_token: &str) -> std::io::Result<Self> {
        Ok(Self {
            process: native_process::ExactProcess::acquire(pid, expected_start_token)?,
            armed: true,
        })
    }

    fn running(&self) -> bool {
        self.process.running()
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ExternalProcessGuard {
    fn drop(&mut self) {
        if self.armed && self.process.running() {
            self.process.terminate_for_cleanup();
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
        }
    }

    fn token(&self) -> &Path {
        &self.token
    }

    fn cleanup(&mut self) {
        let _ = fs::remove_file(&self.token);
        if let Some(cancel) = self.watchdog_cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        let _ = wait_until(|| !endpoint_is_healthy(self.port));
    }

    fn finish(&mut self) {
        self.cleanup();
        self.armed = false;
    }
}

impl Drop for FixtureLifetimeGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cleanup();
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

    fn disarm(&mut self) {
        self.0.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = wait_for_child_exit(child, CHILD_EXIT_GRACE);
        }
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
        let output = run_cli_output_bounded("real `ferric server up`", &mut command);
        let address_in_use = marker_matches(&bind_diagnostic, ADDRESS_IN_USE_DIAGNOSTIC);
        if output.status.success() {
            assert!(
                !bind_diagnostic.exists(),
                "managed fixture reported a bind failure despite successful launch"
            );
            return (port, lifetime);
        }

        lifetime.finish();
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
    state: &Path,
    log: &Path,
) -> (u16, FixtureLifetimeGuard, Output) {
    for attempt in 1..=PORT_ATTEMPTS {
        let port = unused_port();
        let token = root.join(format!("tailscale-managed-fixture-{attempt}.lifetime"));
        let bind_diagnostic = root.join(format!("tailscale-managed-fixture-{attempt}.bind"));
        let mut lifetime = FixtureLifetimeGuard::create(token, port);
        let mut command = isolated_tailscale_ferric(workspace, appdata, bin_dir, state, log);
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
        let output = run_cli_output_bounded("real `ferric server up --tailscale`", &mut command);
        let address_in_use = marker_matches(&bind_diagnostic, ADDRESS_IN_USE_DIAGNOSTIC);
        if output.status.success() {
            assert!(
                !bind_diagnostic.exists(),
                "Tailscale managed fixture reported a bind failure despite successful launch"
            );
            return (port, lifetime, output);
        }

        lifetime.finish();
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
        let child = Command::new(engine)
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
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("could not spawn {label}: {error}"));
        let mut child = ChildGuard::new(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let exit = child
                .child_mut()
                .try_wait()
                .unwrap_or_else(|error| panic!("could not observe {label}: {error}"));
            if let Some(status) = exit {
                let address_in_use = marker_matches(&bind_diagnostic, ADDRESS_IN_USE_DIAGNOSTIC);
                lifetime.finish();
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
                lifetime.finish();
                remove_marker(&bind_diagnostic);
                remove_marker(&ready_marker);
                panic!("{label} exceeded the bounded readiness watchdog");
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    unreachable!("the diagnosed bind retry loop always returns or panics")
}

struct TailscaleLifecycleEvidence {
    command_log: Vec<serde_json::Value>,
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
    let tailscale = copy_fixture_as(&bin_dir, tailscale_filename());
    assert!(engine.is_file());
    assert!(tailscale.is_file());
    let model = root.path().join("dummy-model.gguf");
    fs::write(&model, b"model-free fixture").unwrap();
    let state_path = root.path().join("tailscale-state.json");
    let log_path = root.path().join("tailscale-commands.jsonl");
    let initial_state = initial_tailscale_state();
    write_tailscale_state(&state_path, &initial_state);

    let doctor_port = unused_port();
    let doctor = run_cli_output_bounded(
        "real `ferric server doctor --tailscale`",
        isolated_tailscale_ferric(&workspace, &appdata, &bin_dir, &state_path, &log_path).args([
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
    assert_eq!(read_tailscale_state(&state_path), initial_state);

    let (port, mut process_lifetime, up) = launch_tailscale_managed_fixture_with_retry(
        root.path(),
        &workspace,
        &appdata,
        &bin_dir,
        &model,
        &state_path,
        &log_path,
    );
    let lifetime_token = process_lifetime.token().to_path_buf();
    let up_stdout = String::from_utf8(up.stdout.clone()).unwrap();
    assert_success("real `ferric server up --tailscale`", up);

    let local = local_dir.join("server.json");
    let global = global_dir.join("server.json");
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

    let active_state = read_tailscale_state(&state_path);
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
        isolated_tailscale_ferric(&workspace, &appdata, &bin_dir, &state_path, &log_path)
            .args(["server", "status"]),
    );
    let status_stdout = String::from_utf8(status.stdout.clone()).unwrap();
    assert_success("real `ferric server status` for Tailscale", status);
    assert!(status_stdout.contains(&local_base_url));
    assert!(status_stdout.contains(&remote_base_url));
    assert!(status_stdout.contains("[tailscale] active"));

    let down = run_cli_output_bounded(
        "real `ferric server down` for Tailscale",
        isolated_tailscale_ferric(&workspace, &appdata, &bin_dir, &state_path, &log_path)
            .args(["server", "down"]),
    );
    let down_stdout = String::from_utf8(down.stdout.clone()).unwrap();
    assert_success("real `ferric server down` for Tailscale", down);
    assert!(down_stdout.contains("[removed] local registration"));
    assert!(down_stdout.contains("[removed] global registration"));
    assert!(down_stdout.contains("[state] stopped managed server"));

    assert!(
        wait_until(|| !fixture_guard.running()),
        "Tailscale up-launched fixture PID {pid} remained alive after down"
    );
    fixture_guard.disarm();
    process_lifetime.finish();
    assert!(wait_until(|| endpoint_is_closed(port)));
    assert!(!lifetime_token.exists());
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
    assert_eq!(
        fs::read_to_string(root.path().join(SENTINEL_NAME)).unwrap(),
        "root"
    );

    let final_state = read_tailscale_state(&state_path);
    assert_eq!(final_state, initial_state);
    let command_log = read_tailscale_command_log(&log_path);
    assert!(command_log.iter().any(|entry| {
        entry["journals_ready_on_apply"].as_bool() == Some(true)
            && entry["argv"].as_array().is_some_and(|argv| {
                argv.last().and_then(serde_json::Value::as_str) == Some(proxy_target.as_str())
            })
    }));

    TailscaleLifecycleEvidence {
        command_log,
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

    assert!(
        wait_until(|| !fixture_guard.running()),
        "up-launched fixture PID {pid} remained alive after down"
    );
    assert!(
        wait_until(|| !endpoint_is_healthy(port)),
        "fixture listener 127.0.0.1:{port} remained healthy after down"
    );
    fixture_guard.disarm();
    process_lifetime.finish();
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
fn tailscale_cli_lifecycle_preserves_unrelated_state() {
    let evidence = run_tailscale_lifecycle_fixture();
    assert_eq!(evidence.final_state, evidence.initial_state);
    assert_eq!(
        evidence.final_state["Web"][format!("{TAILSCALE_FQDN}:443")]["Handlers"]
            [UNRELATED_SERVE_PATH],
        serde_json::json!({"Text": "unrelated handler must survive"})
    );
    assert_eq!(evidence.final_state["FutureState"]["preserved"], true);
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

#[test]
fn tailscale_command_log_contains_no_broad_mutation() {
    let evidence = run_tailscale_lifecycle_fixture();
    assert!(!evidence.command_log.is_empty());

    let expected_set_path = format!("--set-path={}", evidence.mount_path);
    let mut saw_identity = false;
    let mut saw_status = false;
    let mut saw_apply = false;
    let mut saw_off = false;
    for entry in &evidence.command_log {
        let argv = entry["argv"]
            .as_array()
            .expect("fixture command log argv must be an array")
            .iter()
            .map(|argument| {
                argument
                    .as_str()
                    .expect("fixture command log argv entries must be strings")
            })
            .collect::<Vec<_>>();

        assert!(
            !argv
                .iter()
                .any(|argument| { matches!(*argument, "reset" | "set-config" | "--set-path=/") })
        );
        match argv.as_slice() {
            ["whoami", "--json"] => saw_identity = true,
            ["serve", "status", "--json"] => saw_status = true,
            ["serve", "--bg", "--https=443", set_path, "--yes", target]
                if *set_path == expected_set_path && *target == evidence.proxy_target.as_str() =>
            {
                assert_eq!(entry["journals_ready_on_apply"], true);
                saw_apply = true;
            }
            ["serve", "--bg", "--https=443", set_path, "--yes", "off"]
                if *set_path == expected_set_path =>
            {
                saw_off = true;
            }
            _ => panic!("fixture recorded forbidden or unscoped Tailscale argv: {argv:?}"),
        }
    }
    assert!(saw_identity, "fixture command log omitted `whoami --json`");
    assert!(
        saw_status,
        "fixture command log omitted `serve status --json`"
    );
    assert!(
        saw_apply,
        "fixture command log did not contain scoped apply"
    );
    assert!(
        saw_off,
        "fixture command log did not contain matching scoped off"
    );
}

#[test]
fn tailscale_fixture_rejects_apply_without_journals() {
    let _lifecycle_lock = lifecycle_test_lock();
    let root = tempfile::tempdir().unwrap();
    let bin_dir = root.path().join("isolated-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let tailscale = copy_fixture_as(&bin_dir, tailscale_filename());
    let state_path = root.path().join("tailscale-state.json");
    let log_path = root.path().join("tailscale-commands.jsonl");
    let initial_state = initial_tailscale_state();
    write_tailscale_state(&state_path, &initial_state);

    let mount_path = "/_ferric/0123456789abcdef0123456789abcdef";
    let output = Command::new(tailscale)
        .args([
            "serve",
            "--bg",
            "--https=443",
            &format!("--set-path={mount_path}"),
            "--yes",
            "http://127.0.0.1:41417",
        ])
        .env(TAILSCALE_STATE_ENV, &state_path)
        .env(TAILSCALE_LOG_ENV, &log_path)
        .env_remove(TAILSCALE_LOCAL_JOURNAL_ENV)
        .env_remove(TAILSCALE_GLOBAL_JOURNAL_ENV)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("refuses Serve apply without both pre-published ownership journals")
    );
    assert_eq!(read_tailscale_state(&state_path), initial_state);
    let log = read_tailscale_command_log(&log_path);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["journals_ready_on_apply"], false);
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

    let fixture_exited = wait_until(|| fixture.child_mut().try_wait().unwrap().is_some());
    assert!(fixture_exited, "adopted fixture remained alive after down");
    // `try_wait` has reaped the exact child. Disarm immediately, before any
    // later assertion can panic and make Drop call Child::kill on a reused PID.
    fixture.disarm();
    assert!(
        wait_until(|| endpoint_is_closed(port)),
        "adopted fixture listener remained open after down"
    );
    process_lifetime.finish();
    assert!(!process_lifetime_token.exists());
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
}
