//! Source-defined subprocess regressions. Cargo starts this harness; recursive
//! fixture modes remain source tests, never separately launched target artifacts.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::{CLEANUP_TIMEOUT, CapturePlan, ProcessTree, enable_subreaper, run_bounded};

const FIXTURE_LIMIT: Duration = Duration::from_secs(45);
const READY_LIMIT: Duration = Duration::from_secs(10);
const LARGE_OUTPUT: usize = 256 * 1024;

fn fixture_command(mode: &str, directory: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("locate source test harness"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    command
        .args([
            "--exact",
            "tests::process_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FERRIC_PROCESS_FIXTURE_MODE", mode)
        .env("FERRIC_PROCESS_FIXTURE_DIRECTORY", directory)
        .stdin(Stdio::null());
    command
}

fn publish(directory: &Path, name: &str, value: &str) {
    let mut staged = tempfile::NamedTempFile::new_in(directory).expect("stage fixture readiness");
    staged
        .write_all(value.as_bytes())
        .expect("write fixture readiness");
    staged.flush().expect("flush fixture readiness");
    staged
        .persist(directory.join(name))
        .expect("publish fixture readiness");
}

fn await_file(path: &Path, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        match std::fs::read_to_string(path) {
            Ok(value) => return value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!("read fixture rendezvous {}: {error}", path.display()),
        }
        assert!(
            Instant::now() < deadline,
            "fixture rendezvous timed out: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A raw fixture child deliberately inherits its leader's existing scope. It
/// must not create a new ProcessTree/group. Before the outer observer accepts
/// handoff, this guard provides bounded cleanup for fixture setup failures.
struct InheritedChild(Option<Child>);

impl Drop for InheritedChild {
    fn drop(&mut self) {
        let Some(child) = self.0.as_mut() else {
            return;
        };
        if let Err(error) = child.kill() {
            match child.try_wait() {
                Ok(Some(_)) => return,
                _ => crate::abort_on_cleanup_failure("fixture child termination failed", error),
            }
        }
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Ok(None) => crate::abort_on_cleanup_failure(
                    "fixture child reaping failed",
                    "deadline exceeded",
                ),
                Err(error) => {
                    crate::abort_on_cleanup_failure("fixture child reaping failed", error)
                }
            }
        }
    }
}

#[test]
#[ignore = "source fixture invoked only by bounded process-scope regressions"]
fn process_fixture() {
    let mode = std::env::var("FERRIC_PROCESS_FIXTURE_MODE").expect("fixture mode required");
    let directory = PathBuf::from(
        std::env::var_os("FERRIC_PROCESS_FIXTURE_DIRECTORY").expect("fixture directory required"),
    );
    match mode.as_str() {
        "holding" => {
            publish(&directory, "ready", &std::process::id().to_string());
            await_file(&directory.join("release"), FIXTURE_LIMIT);
        }
        "descendant" => {
            // Both inherited capture handles remain open until scope cleanup.
            println!("descendant_stdout_open");
            eprintln!("descendant_stderr_open");
            io::stdout().flush().unwrap();
            io::stderr().flush().unwrap();
            publish(
                &directory,
                "descendant-ready",
                &std::process::id().to_string(),
            );
            std::thread::sleep(FIXTURE_LIMIT);
        }
        "leader" => {
            let mut command = fixture_command("descendant", &directory);
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
            let mut descendant = InheritedChild(Some(
                command.spawn().expect("spawn inherited fixture child"),
            ));
            await_file(&directory.join("descendant-ready"), READY_LIMIT);
            publish(&directory, "leader-ready", &std::process::id().to_string());
            // This release is written only after the outer scope has acquired
            // and checked the descendant's exact native identity while live.
            await_file(&directory.join("release"), READY_LIMIT);
            drop(descendant.0.take()); // ownership deliberately stays in the outer Job/group
        }
        "noisy" => {
            let mut stdout = io::stdout().lock();
            let mut stderr = io::stderr().lock();
            stdout.write_all(b"OUT_HEAD\n").unwrap();
            stderr.write_all(b"ERR_HEAD\n").unwrap();
            stdout.write_all(&vec![b'O'; LARGE_OUTPUT]).unwrap();
            stderr.write_all(&vec![b'E'; LARGE_OUTPUT]).unwrap();
            stdout.write_all(b"\nOUT_TAIL\n").unwrap();
            stderr.write_all(b"\nERR_TAIL\n").unwrap();
            stdout.flush().unwrap();
            stderr.flush().unwrap();
        }
        #[cfg(windows)]
        "late-admission-owner" => {
            let mut command = fixture_command("late-admission-leader", &directory);
            command.stdout(Stdio::null()).stderr(Stdio::null());
            let mut leader = ProcessTree::spawn(&mut command).expect("own admission-race leader");
            await_file(&directory.join("late-leader-ready"), READY_LIMIT);
            leader
                .scope
                .as_mut()
                .expect("armed fixture scope")
                .cleanup_checked(|| {
                    // This child is admitted strictly after the inner Job's
                    // exact-handle snapshot, not merely concurrently by chance.
                    publish(&directory, "late-admission-start", "snapshot retained");
                    await_file(&directory.join("descendant-ready"), READY_LIMIT);
                    await_file(&directory.join("late-admission-retained"), READY_LIMIT);
                    Ok(())
                })
                .expect("inner cleanup must reject incomplete identity proof");
            panic!("post-snapshot admission was incorrectly accepted");
        }
        #[cfg(windows)]
        "late-admission-leader" => {
            publish(
                &directory,
                "late-leader-ready",
                &std::process::id().to_string(),
            );
            await_file(&directory.join("late-admission-start"), READY_LIMIT);
            let mut command = fixture_command("descendant", &directory);
            command.stdout(Stdio::null()).stderr(Stdio::null());
            let _descendant = InheritedChild(Some(
                command.spawn().expect("admit post-snapshot descendant"),
            ));
            std::thread::sleep(FIXTURE_LIMIT);
        }
        other => panic!("unknown source fixture mode: {other}"),
    }
}

#[cfg(windows)]
struct ExactProcess(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl ExactProcess {
    fn acquire(pid: u32) -> Self {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        assert!(
            !handle.is_null(),
            "retain exact fixture process: {}",
            io::Error::last_os_error()
        );
        Self(handle)
    }

    fn assert_running(&self) {
        assert_eq!(
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 0) },
            windows_sys::Win32::Foundation::WAIT_TIMEOUT
        );
    }

    fn assert_reaped(&self, role: &str) {
        // Windows has no POSIX zombie wait state. The retained process object
        // proves termination; ProcessTree additionally proves Job Active=0.
        assert_eq!(
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 0) },
            windows_sys::Win32::Foundation::WAIT_OBJECT_0,
            "exact retained {role} remained live after scope cleanup returned"
        );
    }

    fn wait_exit_code(&self, timeout: Duration) -> io::Result<u32> {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        let wait = unsafe {
            WaitForSingleObject(
                self.0,
                timeout.as_millis().try_into().unwrap_or(u32::MAX - 1),
            )
        };
        if wait != WAIT_OBJECT_0 {
            return Err(io::Error::other(format!(
                "exact fixture exit was not observed within deadline: {wait}"
            )));
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.0, &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(code)
    }
}

#[cfg(windows)]
impl Drop for ExactProcess {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(target_os = "linux")]
struct ExactProcess(std::os::fd::OwnedFd);

#[cfg(target_os = "linux")]
impl ExactProcess {
    fn acquire(pid: u32) -> Self {
        use std::os::fd::FromRawFd;
        let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) };
        assert!(
            descriptor >= 0,
            "retain exact fixture pidfd: {}",
            io::Error::last_os_error()
        );
        Self(unsafe { std::os::fd::OwnedFd::from_raw_fd(descriptor as libc::c_int) })
    }

    fn state(&self) -> crate::LinuxProcessState {
        use std::os::fd::AsRawFd;
        let mut descriptor = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            if unsafe { libc::poll(&mut descriptor, 1, 0) } >= 0 {
                return crate::decode_pidfd_events(descriptor.revents)
                    .expect("valid exact fixture pidfd events");
            }
            assert_eq!(
                io::Error::last_os_error().kind(),
                io::ErrorKind::Interrupted
            );
        }
    }

    fn assert_running(&self) {
        assert_eq!(self.state(), crate::LinuxProcessState::Running);
    }
    fn assert_reaped(&self, role: &str) {
        assert_eq!(
            self.state(),
            crate::LinuxProcessState::Reaped,
            "{role}: exit-only POLLIN is not reaping evidence"
        );
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn holding_tree(directory: &Path) -> (ProcessTree, ExactProcess) {
    let mut command = fixture_command("holding", directory);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let tree = ProcessTree::spawn(&mut command).expect("spawn owned fixture");
    let published_pid: u32 = await_file(&directory.join("ready"), READY_LIMIT)
        .parse()
        .unwrap();
    assert_eq!(published_pid, tree.child().id());
    let exact = ExactProcess::acquire(published_pid);
    exact.assert_running();
    (tree, exact)
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn scope_cleanup_success_timeout_unwind() {
    enable_subreaper().expect("explicit controlled-test adoption policy");
    let successful = tempfile::tempdir().unwrap();
    let (mut tree, exact) = holding_tree(successful.path());
    publish(successful.path(), "release", "outer observer ready");
    let start = Instant::now();
    assert!(tree.wait_for_exit(READY_LIMIT).unwrap().success());
    assert!(start.elapsed() < READY_LIMIT + CLEANUP_TIMEOUT + Duration::from_secs(1));
    exact.assert_reaped("successful leader");

    let timeout = tempfile::tempdir().unwrap();
    let (mut tree, exact) = holding_tree(timeout.path());
    let execution_limit = Duration::from_millis(75);
    let start = Instant::now();
    assert_eq!(
        tree.wait_for_exit(execution_limit).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(start.elapsed() < execution_limit + CLEANUP_TIMEOUT + Duration::from_secs(1));
    exact.assert_reaped("timed-out leader");

    let unwinding = tempfile::tempdir().unwrap();
    let (tree, exact) = holding_tree(unwinding.path());
    let start = Instant::now();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _owned_scope = tree;
        panic!("intentional unwind exercises ProcessTree::drop");
    }));
    assert!(panic.is_err());
    assert!(start.elapsed() < CLEANUP_TIMEOUT + Duration::from_secs(1));
    exact.assert_reaped("unwound leader");
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn leader_exit_reaps_descendants() {
    enable_subreaper().expect("explicit adoption of controlled orphan descendants");
    let directory = tempfile::tempdir().unwrap();
    std::thread::scope(|threads| {
        // A scoped worker cannot detach on an assertion failure. Its execution
        // timeout plus checked cleanup bounds the automatic unwind join too.
        let worker = threads.spawn(|| {
            let mut command = fixture_command("leader", directory.path());
            run_bounded(&mut command, READY_LIMIT, CapturePlan::head(2048, 2048))
        });
        let descendant_pid = await_file(&directory.path().join("descendant-ready"), READY_LIMIT)
            .parse()
            .unwrap();
        let leader_pid = await_file(&directory.path().join("leader-ready"), READY_LIMIT)
            .parse()
            .unwrap();
        let descendant = ExactProcess::acquire(descendant_pid);
        let leader = ExactProcess::acquire(leader_pid);
        descendant.assert_running();
        leader.assert_running();
        let start = Instant::now();
        publish(
            directory.path(),
            "release",
            "exact live identities retained",
        );
        let outcome = worker
            .join()
            .expect("capture worker panicked")
            .expect("capture scope cleanup");
        assert!(!outcome.timed_out);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(outcome.status.unwrap().success());
        assert!(start.elapsed() < READY_LIMIT + CLEANUP_TIMEOUT + Duration::from_secs(1));
        assert!(String::from_utf8_lossy(&outcome.stdout).contains("descendant_stdout_open"));
        assert!(String::from_utf8_lossy(&outcome.stderr).contains("descendant_stderr_open"));
        leader.assert_reaped("successful leader with a descendant");
        descendant.assert_reaped("inherited-writer descendant");
    });
}

#[test]
#[cfg(windows)]
fn windows_cleanup_rejects_post_snapshot_admission() {
    let directory = tempfile::tempdir().unwrap();
    let mut command = fixture_command("late-admission-owner", directory.path());
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut outer = ProcessTree::spawn(&mut command).expect("own admission-race fixture tree");
    let owner = ExactProcess::acquire(outer.child().id());
    let leader_pid = await_file(&directory.path().join("late-leader-ready"), READY_LIMIT)
        .parse()
        .unwrap();
    let descendant_pid = await_file(&directory.path().join("descendant-ready"), READY_LIMIT)
        .parse()
        .unwrap();
    let leader = ExactProcess::acquire(leader_pid);
    let descendant = ExactProcess::acquire(descendant_pid);
    owner.assert_running();
    leader.assert_running();
    descendant.assert_running();
    let started = Instant::now();
    let mut owner_code = None;
    outer
        .scope
        .as_mut()
        .expect("armed outer scope")
        .cleanup_checked(|| {
            // The outer scope retains all three exact objects BEFORE allowing
            // the inner failure path. Its unchanged admission fence and native
            // waits therefore prove fixture cleanup even when the inner owner
            // intentionally exits 125 without asserting an incomplete proof.
            publish(
                directory.path(),
                "late-admission-retained",
                "outer snapshot owns every admitted fixture",
            );
            owner_code = Some(owner.wait_exit_code(CLEANUP_TIMEOUT)?);
            Ok(())
        })
        .expect("outer scope proves admission-race fixture cleanup");
    assert_eq!(owner_code, Some(125), "inner cleanup must fail closed");
    assert!(started.elapsed() < CLEANUP_TIMEOUT + Duration::from_secs(1));
    owner.assert_reaped("fail-closed cleanup owner");
    leader.assert_reaped("post-snapshot admission leader");
    descendant.assert_reaped("post-snapshot admitted descendant");
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn bounded_capture_head_tail_and_noisy_child() {
    enable_subreaper().expect("explicit controlled-test adoption policy");
    let directory = tempfile::tempdir().unwrap();
    let mut command = fixture_command("noisy", directory.path());
    let head = run_bounded(&mut command, READY_LIMIT, CapturePlan::head(256, 256)).unwrap();
    assert_eq!(head.exit_code, Some(0));
    assert!(!head.timed_out);
    assert_eq!(head.stdout.len(), 256);
    assert_eq!(head.stderr.len(), 256);
    assert!(String::from_utf8_lossy(&head.stdout).contains("OUT_HEAD\n"));
    assert!(!String::from_utf8_lossy(&head.stdout).contains("OUT_TAIL"));
    assert!(head.stderr.starts_with(b"ERR_HEAD\n"));
    assert_eq!(&head.stderr[9..], vec![b'E'; 247]);

    let mut command = fixture_command("noisy", directory.path());
    let tail = run_bounded(&mut command, READY_LIMIT, CapturePlan::stderr_tail(9)).unwrap();
    assert_eq!(tail.exit_code, Some(0));
    assert!(!tail.timed_out);
    assert!(tail.stdout.is_empty());
    assert_eq!(tail.stderr, b"ERR_TAIL\n");

    let mut command = fixture_command("holding", directory.path());
    let deadline = Duration::from_millis(75);
    let timeout = run_bounded(&mut command, deadline, CapturePlan::discard()).unwrap();
    assert!(timeout.timed_out);
    assert!(timeout.exit_code.is_none());
    assert!(timeout.status.is_none());
    assert!(timeout.stdout.is_empty() && timeout.stderr.is_empty());
    assert!(timeout.wall < deadline + Duration::from_secs(1));
}
