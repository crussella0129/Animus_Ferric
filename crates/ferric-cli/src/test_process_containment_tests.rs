//! Source-driven regressions for complete process-tree teardown.

#![cfg(any(windows, target_os = "linux"))]

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::test_process_containment::{
    ContainedChild, abort_on_cleanup_failure, ensure_current_process_tree_is_contained,
};
// Only the Linux parent-death regression arms a child pre-exec signal directly.
#[cfg(target_os = "linux")]
use crate::test_process_containment::configure_command_parent_death_signal;

const PROBE_MODE_ENV: &str = "FERRIC_PROCESS_TREE_PROBE_MODE";
const PROBE_READY_ENV: &str = "FERRIC_PROCESS_TREE_PROBE_READY";
const PROBE_PARENT_ENV: &str = "FERRIC_PROCESS_TREE_PROBE_PARENT";
const PROBE_TEST_NAME: &str =
    "test_process_containment_tests::source_driven_process_tree_regressions";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const NATURAL_FALLBACK: Duration = Duration::from_secs(120);

struct DirectChildGuard(Option<Child>);

impl DirectChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn terminate_and_reap(&mut self) -> std::io::Result<()> {
        let Some(child) = self.0.as_mut() else {
            return Ok(());
        };
        let mut diagnostics = Vec::new();
        match child.try_wait() {
            Ok(Some(_)) => {
                self.0.take();
                return Ok(());
            }
            Ok(None) => {}
            Err(error) => diagnostics.push(format!("initial observation failed: {error}")),
        }
        if let Err(error) = child.kill() {
            diagnostics.push(format!("exact-process termination failed: {error}"));
        }
        let deadline = Instant::now() + PROBE_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.0.take();
                    return if diagnostics.is_empty() {
                        Ok(())
                    } else {
                        Err(std::io::Error::other(diagnostics.join("; ")))
                    };
                }
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!(
                            "source probe did not exit after termination ({})",
                            diagnostics.join("; ")
                        ),
                    ));
                }
                Err(error) => {
                    return Err(std::io::Error::other(format!(
                        "could not reap source probe: {error}; {}",
                        diagnostics.join("; ")
                    )));
                }
            }
        }
    }
}

impl Drop for DirectChildGuard {
    fn drop(&mut self) {
        if let Err(error) = self.terminate_and_reap() {
            abort_on_cleanup_failure("could not clean exact source probe", error);
        }
    }
}

#[cfg(windows)]
mod exact_process {
    use super::{PROBE_TIMEOUT, abort_on_cleanup_failure};
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };

    pub(super) struct ExactProcess(HANDLE);

    fn generation(handle: HANDLE) -> u64 {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        assert_ne!(
            unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) },
            0
        );
        (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
    }

    pub(super) fn current_generation() -> u64 {
        generation(unsafe { GetCurrentProcess() })
    }

    pub(super) fn child_generation(child: &std::process::Child) -> u64 {
        use std::os::windows::io::AsRawHandle;
        generation(child.as_raw_handle().cast())
    }

    impl ExactProcess {
        pub(super) fn open(pid: u32, expected_generation: u64) -> Self {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                    0,
                    pid,
                )
            };
            assert!(
                !handle.is_null(),
                "open exact source probe {pid}: {}",
                std::io::Error::last_os_error()
            );
            // Validate before constructing the signalling cleanup guard. An
            // old readiness file never authorizes killing a replacement PID.
            if generation(handle) != expected_generation {
                unsafe {
                    CloseHandle(handle);
                }
                panic!("source-probe process generation changed");
            }
            Self(handle)
        }

        pub(super) fn wait_for_exit(&self, timeout: Duration) {
            self.wait_for_exit_result(timeout)
                .expect("wait for exact source-probe exit");
        }

        pub(super) fn wait_for_exit_result(&self, timeout: Duration) -> std::io::Result<()> {
            let timeout_ms = u32::try_from(timeout.as_millis()).expect("bounded probe timeout");
            match unsafe { WaitForSingleObject(self.0, timeout_ms) } {
                WAIT_OBJECT_0 => Ok(()),
                WAIT_TIMEOUT => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "exact source probe exit timed out",
                )),
                WAIT_FAILED => Err(std::io::Error::last_os_error()),
                state => Err(std::io::Error::other(format!(
                    "unexpected exact-source-probe wait state {state}"
                ))),
            }
        }

        pub(super) fn terminate_only(&self) {
            if unsafe { WaitForSingleObject(self.0, 0) } != WAIT_OBJECT_0 {
                assert_ne!(
                    unsafe { TerminateProcess(self.0, 1) },
                    0,
                    "terminate exact source-probe owner: {}",
                    std::io::Error::last_os_error()
                );
            }
        }

        pub(super) fn wait_for_reaped(&self, timeout: Duration) {
            self.wait_for_exit(timeout);
        }

        pub(super) fn wait_for_reaped_result(&self, timeout: Duration) -> std::io::Result<()> {
            self.wait_for_exit_result(timeout)
        }

        pub(super) fn observe_leader(child: &std::process::Child) -> Self {
            Self::open(child.id(), child_generation(child))
        }

        pub(super) fn observe_owned_descendant(pid: u32, generation: u64) -> Self {
            Self::open(pid, generation)
        }
    }

    impl Drop for ExactProcess {
        fn drop(&mut self) {
            if unsafe { WaitForSingleObject(self.0, 0) } != WAIT_OBJECT_0 {
                let terminated = unsafe { TerminateProcess(self.0, 1) };
                if terminated == 0 && unsafe { WaitForSingleObject(self.0, 0) } != WAIT_OBJECT_0 {
                    abort_on_cleanup_failure(
                        "could not terminate exact Windows source probe",
                        std::io::Error::last_os_error(),
                    );
                }
                match unsafe { WaitForSingleObject(self.0, PROBE_TIMEOUT.as_millis() as u32) } {
                    WAIT_OBJECT_0 => {}
                    state => abort_on_cleanup_failure(
                        "could not prove exact Windows source-probe cleanup",
                        state,
                    ),
                }
            }
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod exact_process {
    use super::{PROBE_TIMEOUT, abort_on_cleanup_failure};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::time::{Duration, Instant};

    pub(super) struct ExactProcess {
        descriptor: OwnedFd,
        reap_on_drop: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ExitState {
        Running,
        Exited,
        Reaped,
    }

    fn generation(pid: u32) -> u64 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .expect("read source process generation");
        let end = stat.rfind(')').expect("source process command delimiter");
        stat[end + 1..]
            .split_whitespace()
            .nth(19)
            .expect("source process start ticks")
            .parse()
            .expect("numeric source process start ticks")
    }

    pub(super) fn current_generation() -> u64 {
        generation(std::process::id())
    }

    pub(super) fn child_generation(child: &std::process::Child) -> u64 {
        generation(child.id())
    }

    fn decode_events(events: i16) -> std::io::Result<ExitState> {
        if events & !(libc::POLLIN | libc::POLLHUP) != 0 {
            return Err(std::io::Error::other(format!(
                "exact source pidfd returned invalid events {events:#x}"
            )));
        }
        if events & libc::POLLHUP != 0 {
            Ok(ExitState::Reaped)
        } else if events & libc::POLLIN != 0 {
            Ok(ExitState::Exited)
        } else {
            Ok(ExitState::Running)
        }
    }

    #[test]
    fn invalid_pidfd_events_fail_closed() {
        assert_eq!(decode_events(0).unwrap(), ExitState::Running);
        assert_eq!(decode_events(libc::POLLIN).unwrap(), ExitState::Exited);
        assert_eq!(decode_events(libc::POLLHUP).unwrap(), ExitState::Reaped);
        assert_eq!(
            decode_events(libc::POLLIN | libc::POLLHUP).unwrap(),
            ExitState::Reaped
        );
        for invalid in [libc::POLLERR, libc::POLLNVAL, libc::POLLIN | libc::POLLNVAL] {
            assert!(decode_events(invalid).is_err());
        }
    }

    impl ExactProcess {
        pub(super) fn open(pid: u32, expected_generation: u64) -> Self {
            let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            assert!(
                descriptor >= 0,
                "open exact source probe {pid}: {}",
                std::io::Error::last_os_error()
            );
            let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor as libc::c_int) };
            assert_eq!(
                generation(pid),
                expected_generation,
                "source process generation changed"
            );
            Self {
                descriptor,
                reap_on_drop: true,
            }
        }

        pub(super) fn observe_leader(child: &std::process::Child) -> Self {
            let mut process = Self::open(child.id(), child_generation(child));
            process.reap_on_drop = false;
            process
        }

        pub(super) fn observe_owned_descendant(pid: u32, generation: u64) -> Self {
            let mut process = Self::open(pid, generation);
            // The enclosing source scope owns failure-path reaping. Reaping
            // here could precede its still-live parent's orderly shutdown.
            process.reap_on_drop = false;
            process
        }

        fn wait_state(&self, timeout: Duration) -> std::io::Result<ExitState> {
            let mut descriptor = libc::pollfd {
                fd: self.descriptor.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
            let state = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
            if state > 0 {
                decode_events(descriptor.revents)
            } else if state == 0 {
                Ok(ExitState::Running)
            } else {
                Err(std::io::Error::last_os_error())
            }
        }

        pub(super) fn wait_for_exit(&self, timeout: Duration) {
            self.wait_for_exit_result(timeout)
                .expect("wait for exact source-probe exit");
        }

        pub(super) fn wait_for_exit_result(&self, timeout: Duration) -> std::io::Result<()> {
            match self.wait_state(timeout) {
                Ok(ExitState::Exited | ExitState::Reaped) => Ok(()),
                Ok(ExitState::Running) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "exact source probe exit timed out",
                )),
                Err(error) => Err(error),
            }
        }

        fn terminate(&self) -> std::io::Result<()> {
            if self.wait_state(Duration::ZERO)? != ExitState::Running {
                return Ok(());
            }
            let result = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    self.descriptor.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            if result != 0 && self.wait_state(Duration::ZERO)? == ExitState::Running {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn terminate_only(&self) {
            self.terminate().expect("terminate only exact source owner");
        }

        fn reap(&self, timeout: Duration) -> std::io::Result<()> {
            let deadline = Instant::now() + timeout;
            loop {
                if self.wait_state(Duration::ZERO)? == ExitState::Reaped {
                    return Ok(());
                }
                let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PIDFD,
                        self.descriptor.as_raw_fd() as libc::id_t,
                        &mut status,
                        libc::WEXITED | libc::WNOHANG,
                    )
                };
                if result == 0 && unsafe { status.si_pid() } != 0 {
                    return Ok(());
                }
                if result != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ECHILD)
                        && error.kind() != std::io::ErrorKind::Interrupted
                    {
                        return Err(error);
                    }
                }
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "exact source helper was not proved reaped",
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        pub(super) fn wait_for_reaped(&self, timeout: Duration) {
            self.reap(timeout)
                .expect("reap exact adopted source helper");
        }

        pub(super) fn wait_for_reaped_result(&self, timeout: Duration) -> std::io::Result<()> {
            self.reap(timeout)
        }
    }

    impl Drop for ExactProcess {
        fn drop(&mut self) {
            if let Err(error) = self.terminate() {
                abort_on_cleanup_failure("could not terminate exact Linux source probe", error);
            }
            if self.reap_on_drop {
                if let Err(error) = self.reap(PROBE_TIMEOUT) {
                    abort_on_cleanup_failure("could not reap exact Linux source probe", error);
                }
            } else if !matches!(
                self.wait_state(PROBE_TIMEOUT),
                Ok(ExitState::Exited | ExitState::Reaped)
            ) {
                abort_on_cleanup_failure("exact source-probe leader remained live", "exit timeout");
            }
        }
    }
}

use exact_process::ExactProcess;

fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    predicate()
}

fn source_command(mode: &str, ready: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("current Rust test harness"));
    command
        .args([
            "--exact",
            PROBE_TEST_NAME,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PROBE_MODE_ENV, mode)
        .env(PROBE_READY_ENV, ready)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn spawn_raw_source(mode: &str, ready: &Path) -> Child {
    source_command(mode, ready)
        .spawn()
        .expect("spawn source-defined exact probe")
}

fn ready_path() -> PathBuf {
    std::env::var_os(PROBE_READY_ENV)
        .map(PathBuf::from)
        .expect("source probe ready path")
}

fn publish(path: &Path, value: &str) {
    let staged = path.with_extension(format!("staged-{}", std::process::id()));
    fs::write(&staged, value).expect("stage complete source-probe coordinates");
    fs::rename(&staged, path).expect("publish source-probe coordinates atomically");
}

fn parse_listener(path: &Path) -> Option<(u32, u64, u16)> {
    let value = fs::read_to_string(path).ok()?;
    let mut fields = value.split_whitespace();
    let version = fields.next()?;
    let pid = fields.next()?.parse::<u32>().ok()?;
    let generation = fields.next()?.parse::<u64>().ok()?;
    let port = fields.next()?.parse::<u16>().ok()?;
    (version == "v2" && pid != 0 && generation != 0 && port != 0 && fields.next().is_none())
        .then_some((pid, generation, port))
}

fn parse_watched(path: &Path) -> Option<(u32, u64, u32, u64, u16)> {
    let value = fs::read_to_string(path).ok()?;
    let mut fields = value.split_whitespace();
    let version = fields.next()?;
    let harness = fields.next()?.parse::<u32>().ok()?;
    let harness_generation = fields.next()?.parse::<u64>().ok()?;
    let listener = fields.next()?.parse::<u32>().ok()?;
    let listener_generation = fields.next()?.parse::<u64>().ok()?;
    let port = fields.next()?.parse::<u16>().ok()?;
    (version == "v2"
        && harness != 0
        && harness_generation != 0
        && listener != 0
        && listener_generation != 0
        && port != 0
        && fields.next().is_none())
    .then_some((
        harness,
        harness_generation,
        listener,
        listener_generation,
        port,
    ))
}

fn run_listener() -> ! {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind source-probe listener");
    let port = listener.local_addr().expect("source-probe address").port();
    publish(
        &ready_path(),
        &format!(
            "v2 {} {} {port}",
            std::process::id(),
            exact_process::current_generation()
        ),
    );
    thread::sleep(NATURAL_FALLBACK);
    std::process::exit(0);
}

fn run_tree_parent() -> ! {
    let _listener = DirectChildGuard::new(spawn_raw_source("listener", &ready_path()));
    thread::sleep(NATURAL_FALLBACK);
    panic!("tree parent reached its natural fallback");
}

fn run_watched_harness() -> ! {
    #[cfg(target_os = "linux")]
    let expected_parent = std::env::var(PROBE_PARENT_ENV)
        .expect("source supervisor identity")
        .parse::<libc::pid_t>()
        .expect("numeric source supervisor identity");
    #[cfg(target_os = "linux")]
    if unsafe { libc::getppid() } != expected_parent {
        std::process::exit(0);
    }
    ensure_current_process_tree_is_contained()
        .expect("install exact-parent watcher in source-defined harness");
    let root_ready = ready_path();
    // The supervisor must publish our direct-child coordinate before this
    // process may create a nested scope. Failure cleanup can then give this
    // retained watcher time to drain that scope before touching the outer one.
    let watched_pid = root_ready.with_extension("watched-pid");
    assert!(
        wait_until(PROBE_TIMEOUT, || {
            fs::read_to_string(&watched_pid).ok().and_then(|value| {
                let mut fields = value.split_whitespace();
                Some((
                    fields.next()?.parse::<u32>().ok()?,
                    fields.next()?.parse::<u64>().ok()?,
                ))
            }) == Some((std::process::id(), exact_process::current_generation()))
        }),
        "supervisor did not acknowledge watched harness ownership"
    );
    #[cfg(target_os = "linux")]
    if unsafe { libc::getppid() } != expected_parent {
        std::process::exit(0);
    }
    let listener_ready = root_ready.with_extension("listener-ready");
    let mut command = source_command("listener", &listener_ready);
    let _listener =
        ContainedChild::spawn(&mut command).expect("spawn contained watched-harness listener");
    let mut coordinates = None;
    assert!(
        wait_until(PROBE_TIMEOUT, || {
            coordinates = parse_listener(&listener_ready);
            coordinates.is_some()
        }),
        "watched harness listener did not become ready"
    );
    let (listener_pid, listener_generation, port) =
        coordinates.expect("complete listener coordinates");
    publish(
        &root_ready,
        &format!(
            "v2 {} {} {listener_pid} {listener_generation} {port}",
            std::process::id(),
            exact_process::current_generation()
        ),
    );
    thread::sleep(NATURAL_FALLBACK);
    panic!("watched harness reached its natural fallback");
}

fn run_supervisor() -> ! {
    let ready = ready_path();
    let mut command = source_command("watched-harness", &ready);
    command.env(PROBE_PARENT_ENV, std::process::id().to_string());
    let watched = command.spawn().expect("spawn acknowledged source watcher");
    let watched_pid = watched.id();
    let watched_generation = exact_process::child_generation(&watched);
    let _watched = DirectChildGuard::new(watched);
    publish(
        &ready.with_extension("watched-pid"),
        &format!("{watched_pid} {watched_generation}"),
    );
    thread::sleep(NATURAL_FALLBACK);
    panic!("source supervisor reached its natural fallback");
}

#[cfg(target_os = "linux")]
fn run_pdeath_supervisor() -> ! {
    let mut command = source_command("listener", &ready_path());
    configure_command_parent_death_signal(&mut command)
        .expect("arm pre-exec Linux parent-death signal");
    let _listener =
        DirectChildGuard::new(command.spawn().expect("spawn Linux parent-death listener"));
    thread::sleep(NATURAL_FALLBACK);
    panic!("Linux parent-death supervisor reached its natural fallback");
}

fn assert_listener_closes(pid: u32, generation: u64, port: u16, terminate: impl FnOnce()) {
    let process = ExactProcess::observe_owned_descendant(pid, generation);
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_ok(),
        "coordinates preceded listener readiness"
    );
    terminate();
    process.wait_for_exit(PROBE_TIMEOUT);
    process.wait_for_reaped(PROBE_TIMEOUT);
    assert!(
        wait_until(PROBE_TIMEOUT, || {
            TcpStream::connect(("127.0.0.1", port)).is_err()
        }),
        "source-probe listener remained open after exact process exit"
    );
}

fn verify_per_child_tree_cleanup(root: &Path) {
    let ready = root.join("tree.ready");
    let mut command = source_command("tree-parent", &ready);
    let mut parent = ContainedChild::spawn(&mut command).expect("spawn contained tree parent");
    let mut coordinates = None;
    assert!(
        wait_until(PROBE_TIMEOUT, || {
            coordinates = parse_listener(&ready);
            coordinates.is_some()
        }),
        "tree parent did not publish descendant coordinates"
    );
    let (pid, generation, port) = coordinates.expect("complete tree coordinates");
    assert_listener_closes(pid, generation, port, || {
        parent
            .terminate_and_reap()
            .expect("terminate and reap complete source-defined child tree");
    });
}

struct SupervisorGuard {
    scope: ContainedChild,
    watched_pid: PathBuf,
    watched: Option<ExactProcess>,
    finished: bool,
}

impl SupervisorGuard {
    fn cleanup(&mut self) -> std::io::Result<()> {
        if self.finished {
            return Ok(());
        }
        // Killing only the upstream owner lets the watched harness drain its
        // nested scopes. Killing this outer group first would kill the watcher.
        self.scope.terminate_leader()?;
        let deadline = Instant::now() + PROBE_TIMEOUT;
        while self.scope.try_wait_leader()?.is_none() {
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "upstream source owner did not exit",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
        if self.watched.is_none()
            && let Ok(value) = fs::read_to_string(&self.watched_pid)
        {
            let mut fields = value.split_whitespace();
            let pid = fields
                .next()
                .ok_or_else(|| std::io::Error::other("missing watched PID"))?
                .parse::<u32>()
                .map_err(std::io::Error::other)?;
            let generation = fields
                .next()
                .ok_or_else(|| std::io::Error::other("missing watched generation"))?
                .parse::<u64>()
                .map_err(std::io::Error::other)?;
            self.watched = Some(ExactProcess::open(pid, generation));
        }
        if let Some(watched) = &self.watched {
            watched.wait_for_exit_result(PROBE_TIMEOUT)?;
            watched.wait_for_reaped_result(PROBE_TIMEOUT)?;
        }
        self.scope.terminate_and_reap()?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for SupervisorGuard {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            abort_on_cleanup_failure(
                "source supervisor could not drain its watched scopes",
                error,
            );
        }
    }
}

fn verify_upstream_supervisor_cleanup(root: &Path) {
    let ready = root.join("supervisor.ready");
    let scope = ContainedChild::spawn(&mut source_command("supervisor", &ready))
        .expect("spawn contained source supervisor");
    let mut supervisor = SupervisorGuard {
        scope,
        watched_pid: ready.with_extension("watched-pid"),
        watched: None,
        finished: false,
    };
    let mut coordinates = None;
    assert!(
        wait_until(PROBE_TIMEOUT, || {
            coordinates = parse_watched(&ready);
            coordinates.is_some()
        }),
        "watched harness did not publish complete coordinates"
    );
    let (harness_pid, harness_generation, listener_pid, listener_generation, port) =
        coordinates.expect("complete watched-harness coordinates");
    supervisor.watched = Some(ExactProcess::open(harness_pid, harness_generation));
    let owner = ExactProcess::observe_leader(supervisor.scope.child());
    assert_listener_closes(listener_pid, listener_generation, port, || {
        owner.terminate_only();
    });
    let harness = supervisor
        .watched
        .as_ref()
        .expect("retained watched harness");
    harness.wait_for_exit(PROBE_TIMEOUT);
    harness.wait_for_reaped(PROBE_TIMEOUT);
    supervisor.cleanup().expect("drain source supervisor scope");
}

#[cfg(target_os = "linux")]
fn verify_linux_parent_death_signal(root: &Path) {
    let ready = root.join("pdeath.ready");
    let mut supervisor = ContainedChild::spawn(&mut source_command("pdeath-supervisor", &ready))
        .expect("spawn contained Linux parent-death supervisor");
    let mut coordinates = None;
    assert!(
        wait_until(PROBE_TIMEOUT, || {
            coordinates = parse_listener(&ready);
            coordinates.is_some()
        }),
        "Linux parent-death listener did not publish coordinates"
    );
    let (pid, generation, port) = coordinates.expect("complete Linux parent-death coordinates");
    let owner = ExactProcess::observe_leader(supervisor.child());
    assert_listener_closes(pid, generation, port, || {
        owner.terminate_only();
    });
    supervisor
        .terminate_and_reap()
        .expect("reap Linux parent-death supervisor scope");
}

#[test]
fn source_driven_process_tree_regressions() {
    match std::env::var(PROBE_MODE_ENV).ok().as_deref() {
        Some("listener") => run_listener(),
        Some("tree-parent") => run_tree_parent(),
        Some("watched-harness") => run_watched_harness(),
        Some("supervisor") => run_supervisor(),
        #[cfg(target_os = "linux")]
        Some("pdeath-supervisor") => run_pdeath_supervisor(),
        Some(mode) => panic!("unknown source process-tree probe mode {mode:?}"),
        None => {}
    }

    ensure_current_process_tree_is_contained().expect("install source-test harness containment");
    let root = tempfile::tempdir().expect("create source process-tree probe directory");
    verify_per_child_tree_cleanup(root.path());
    #[cfg(target_os = "linux")]
    verify_linux_parent_death_signal(root.path());
}

#[test]
fn parent_watch_retains_identity() {
    ensure_current_process_tree_is_contained().expect("install source-test parent containment");
    let root = tempfile::tempdir().expect("create exact-parent source regression directory");
    verify_upstream_supervisor_cleanup(root.path());
}
