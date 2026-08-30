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

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const SENTINEL_NAME: &str = "unrelated-sentinel.txt";

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

fn assert_success(label: &str, output: Output) {
    assert!(
        output.status.success(),
        "{label} failed ({}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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

fn write_bytes(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
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
    armed: bool,
}

impl FixtureLifetimeGuard {
    fn create(token: PathBuf, port: u16) -> Self {
        fs::write(&token, b"fixture may run only while this token exists").unwrap();
        Self {
            token,
            port,
            armed: true,
        }
    }

    fn cleanup(&self) {
        let _ = fs::remove_file(&self.token);
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
            let _ = child.wait();
        }
    }
}

#[test]
fn model_free_server_lifecycle_fixture_e2e() {
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
    let port = unused_port();

    // Null output is load-bearing: the engine inherits Ferric's stdio, so an
    // output pipe owned by `ferric server up` would stay open in the daemon.
    let up = isolated_ferric(&workspace_b, &appdata, &bin_dir)
        .args([
            "server",
            "up",
            "--model",
            model.to_str().unwrap(),
            "--ctx",
            "4096",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(up.success(), "real `ferric server up` failed: {up}");

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

    assert_success(
        "status from originating workspace B",
        isolated_ferric(&workspace_b, &appdata, &bin_dir)
            .args(["server", "status"])
            .output()
            .unwrap(),
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
    stale["process_identity"]["start_token"] =
        serde_json::json!(format!("{old_token}-deliberately-stale"));
    stale["origin_local_runfile"] = serde_json::json!(local_a.to_string_lossy().into_owned());
    write_bytes(&local_a, &serde_json::to_vec_pretty(&stale).unwrap());

    assert_success(
        "status from stale workspace A",
        isolated_ferric(&workspace_a, &appdata, &bin_dir)
            .args(["server", "status"])
            .output()
            .unwrap(),
    );
    assert_success(
        "down from stale workspace A",
        isolated_ferric(&workspace_a, &appdata, &bin_dir)
            .args(["server", "down"])
            .output()
            .unwrap(),
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
    assert_only_sentinel(&local_a_dir, "workspace-a");
    assert_only_sentinel(&local_b_dir, "workspace-b");
    assert_only_sentinel(&global_dir, "global");
    assert_eq!(
        fs::read_to_string(root.path().join(SENTINEL_NAME)).unwrap(),
        "root"
    );
}

#[test]
fn tailscale_refusal_has_zero_external_effects() {
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
    assert!(engine.is_file(), "fake closed engine must exist");
    assert!(tailscale.is_file(), "fake tailscale executable must exist");
    let model = root.path().join("dummy-model.gguf");
    fs::write(&model, b"model-free fixture").unwrap();
    let port = unused_port();
    assert!(endpoint_is_closed(port));

    let invocation_marker = root.path().join("unexpected-invocation.json");
    let lifetime_token = root.path().join("fixture-lifetime.token");
    let mut lifetime = FixtureLifetimeGuard::create(lifetime_token.clone(), port);

    // Null stdio prevents a hypothetical unexpectedly spawned engine from
    // keeping an output pipe open. The lifetime token makes such a fixture
    // self-terminate safely if any assertion below exposes a regression.
    let status = isolated_ferric(&workspace, &appdata, &bin_dir)
        .env(
            "FERRIC_LIFECYCLE_FIXTURE_INVOCATION_MARKER",
            &invocation_marker,
        )
        .env("FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN", &lifetime_token)
        .args([
            "server",
            "up",
            "--tailscale",
            "--model",
            model.to_str().unwrap(),
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();

    assert!(
        !status.success(),
        "`server up --tailscale` must fail while scoped Serve ownership is unavailable"
    );
    assert!(
        !invocation_marker.exists(),
        "engine or tailscale subprocess was invoked before refusal: {}",
        invocation_marker.display()
    );
    assert!(
        endpoint_is_closed(port),
        "refused tailscale launch created a listener on 127.0.0.1:{port}"
    );
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
    assert_eq!(
        fs::read_to_string(root.path().join(SENTINEL_NAME)).unwrap(),
        "root"
    );

    lifetime.finish();
    assert!(!lifetime_token.exists());
}

#[test]
fn legacy_adoption_then_down_cli_e2e() {
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
    let port = unused_port();

    let child = Command::new(&engine)
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
        .current_dir(&workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut fixture = ChildGuard::new(child);
    assert!(
        wait_until(|| endpoint_is_healthy(port)),
        "direct lifecycle fixture did not become healthy"
    );

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

    assert_success(
        "legacy adoption",
        isolated_ferric(&workspace, &appdata, &bin_dir)
            .args(["server", "adopt", "--pid", &pid.to_string()])
            .output()
            .unwrap(),
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
        isolated_ferric(&workspace, &appdata, &bin_dir)
            .args(["server", "status"])
            .output()
            .unwrap(),
    );
    assert_success(
        "down after adoption",
        isolated_ferric(&workspace, &appdata, &bin_dir)
            .args(["server", "down"])
            .output()
            .unwrap(),
    );

    let fixture_exited = wait_until(|| fixture.child_mut().try_wait().unwrap().is_some());
    assert!(fixture_exited, "adopted fixture remained alive after down");
    // `try_wait` has reaped the exact child. Disarm immediately, before any
    // later assertion can panic and make Drop call Child::kill on a reused PID.
    fixture.disarm();
    assert!(
        wait_until(|| !endpoint_is_healthy(port)),
        "adopted fixture listener remained healthy after down"
    );
    assert_only_sentinel(&local_dir, "workspace");
    assert_only_sentinel(&global_dir, "global");
}
