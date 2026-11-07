//! Source-defined subprocess regressions. Cargo starts this harness; recursive
//! fixture modes remain source tests, never separately launched target artifacts.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::exact::{Access, ExactProcess, ProcessState};
use crate::{CLEANUP_TIMEOUT, CapturePlan, ProcessTree, enable_subreaper, run_bounded};

const FIXTURE_LIMIT: Duration = Duration::from_secs(45);
const READY_LIMIT: Duration = Duration::from_secs(10);
const LARGE_OUTPUT: usize = 256 * 1024;

#[test]
pub(crate) fn cleanup_deadline_precedes_success() {
    let observed_at = Instant::now();
    let deadline = observed_at + Duration::from_millis(1);
    assert!(crate::cleanup_complete(true, observed_at, deadline).unwrap());
    assert!(!crate::cleanup_complete(false, observed_at, deadline).unwrap());
    for drained in [false, true] {
        for expired_at in [deadline, deadline + Duration::from_millis(1)] {
            assert_eq!(
                crate::cleanup_complete(drained, expired_at, deadline)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::TimedOut,
                "the deadline wins even when the scope is already drained"
            );
        }
    }
}

fn fixture_command(mode: &str, directory: &Path) -> Command {
    let mut command = crate::testing::harness_command("tests::process_fixture");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    command
        .arg("--ignored")
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
        "listener" => {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind listener");
            let port = listener.local_addr().expect("listener address").port();
            let pid = std::process::id();
            publish(
                &directory,
                "listener",
                &format!("{pid} {} {port}", generation(pid)),
            );
            std::thread::sleep(FIXTURE_LIMIT);
        }
        "tree-parent" => {
            let _listener = InheritedChild(Some(
                fixture_command("listener", &directory)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn inherited listener"),
            ));
            std::thread::sleep(FIXTURE_LIMIT);
        }
        "supervisor" => {
            let mut command = fixture_command("watched-harness", &directory);
            command
                .env(
                    "FERRIC_PROCESS_FIXTURE_PARENT",
                    std::process::id().to_string(),
                )
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let watched = InheritedChild(Some(command.spawn().expect("spawn watched harness")));
            let pid = watched.0.as_ref().expect("watched harness").id();
            publish(&directory, "watched", &format!("{pid} {}", generation(pid)));
            std::thread::sleep(FIXTURE_LIMIT);
        }
        "watched-harness" => {
            // Watching a parent that already died would watch the subreaper
            // instead, so a harness that lost its supervisor leaves at once.
            #[cfg(target_os = "linux")]
            let supervised = || {
                std::env::var("FERRIC_PROCESS_FIXTURE_PARENT").ok()
                    == Some(unsafe { libc::getppid() }.to_string())
            };
            #[cfg(target_os = "linux")]
            if !supervised() {
                return;
            }
            crate::testing::contain_current_process().expect("watch the exact supervisor");
            // The supervisor publishes this harness's coordinate before it may
            // create a nested scope, so failure cleanup can let this retained
            // watcher drain that scope before touching the outer one.
            let pid = std::process::id();
            assert_eq!(
                await_file(&directory.join("watched"), READY_LIMIT),
                format!("{pid} {}", generation(pid)),
                "supervisor did not acknowledge watched harness ownership"
            );
            #[cfg(target_os = "linux")]
            if !supervised() {
                return;
            }
            let mut command = fixture_command("listener", &directory);
            command.stdout(Stdio::null()).stderr(Stdio::null());
            let _listener = crate::testing::spawn(&mut command).expect("own nested listener");
            await_file(&directory.join("listener"), READY_LIMIT);
            publish(&directory, "watched-ready", "nested listener owned");
            std::thread::sleep(FIXTURE_LIMIT);
        }
        #[cfg(target_os = "linux")]
        "pdeath-supervisor" => {
            let mut command = fixture_command("listener", &directory);
            command.stdout(Stdio::null()).stderr(Stdio::null());
            crate::testing::parent_death_signal(&mut command);
            let _listener = InheritedChild(Some(command.spawn().expect("spawn pdeath listener")));
            std::thread::sleep(FIXTURE_LIMIT);
        }
        other => panic!("unknown source fixture mode: {other}"),
    }
}

fn generation(pid: u32) -> u64 {
    observe(pid)
        .generation()
        .expect("read exact process generation")
}

/// Retain a published fixture process, refusing a replacement whose start
/// generation differs from the published one.
fn observe_published(coordinates: &str) -> ExactProcess {
    let mut fields = coordinates.split_whitespace();
    let pid = fields
        .next()
        .and_then(|pid| pid.parse().ok())
        .expect("published PID");
    let published = fields
        .next()
        .and_then(|generation| generation.parse().ok())
        .expect("published generation");
    let process = observe(pid);
    assert_eq!(
        process.generation().expect("read retained generation"),
        published,
        "fixture process generation changed"
    );
    process
}

/// Retain `pid`'s exact process object for observation only.
fn observe(pid: u32) -> ExactProcess {
    ExactProcess::open(pid, Access::Observe).expect("retain exact fixture process")
}

fn assert_running(process: &ExactProcess) {
    assert_eq!(
        process.state(Duration::ZERO).unwrap(),
        ProcessState::Running
    );
}

/// On Linux an exit-only (zombie) observation is not reaping evidence. Windows
/// has no zombie state; ProcessTree additionally proves Job Active=0.
fn assert_reaped(process: &ExactProcess, role: &str) {
    assert_eq!(
        process.state(Duration::ZERO).unwrap(),
        ProcessState::Reaped,
        "exact retained {role} was not reaped when scope cleanup returned"
    );
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
    let exact = observe(published_pid);
    assert_running(&exact);
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
    assert_reaped(&exact, "successful leader");

    let timeout = tempfile::tempdir().unwrap();
    let (mut tree, exact) = holding_tree(timeout.path());
    let execution_limit = Duration::from_millis(75);
    let start = Instant::now();
    assert_eq!(
        tree.wait_for_exit(execution_limit).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(start.elapsed() < execution_limit + CLEANUP_TIMEOUT + Duration::from_secs(1));
    assert_reaped(&exact, "timed-out leader");

    let unwinding = tempfile::tempdir().unwrap();
    let (tree, exact) = holding_tree(unwinding.path());
    let start = Instant::now();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _owned_scope = tree;
        panic!("intentional unwind exercises ProcessTree::drop");
    }));
    assert!(panic.is_err());
    assert!(start.elapsed() < CLEANUP_TIMEOUT + Duration::from_secs(1));
    assert_reaped(&exact, "unwound leader");
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
        let descendant = observe(descendant_pid);
        let leader = observe(leader_pid);
        assert_running(&descendant);
        assert_running(&leader);
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
        assert_reaped(&leader, "successful leader with a descendant");
        assert_reaped(&descendant, "inherited-writer descendant");
    });
}

#[test]
#[cfg(windows)]
fn windows_cleanup_rejects_post_snapshot_admission() {
    let directory = tempfile::tempdir().unwrap();
    let mut command = fixture_command("late-admission-owner", directory.path());
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut outer = ProcessTree::spawn(&mut command).expect("own admission-race fixture tree");
    let owner = observe(outer.child().id());
    let leader_pid = await_file(&directory.path().join("late-leader-ready"), READY_LIMIT)
        .parse()
        .unwrap();
    let descendant_pid = await_file(&directory.path().join("descendant-ready"), READY_LIMIT)
        .parse()
        .unwrap();
    let leader = observe(leader_pid);
    let descendant = observe(descendant_pid);
    assert_running(&owner);
    assert_running(&leader);
    assert_running(&descendant);
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
            owner.reap(CLEANUP_TIMEOUT)?;
            owner_code = Some(owner.exit_code()?);
            Ok(())
        })
        .expect("outer scope proves admission-race fixture cleanup");
    assert_eq!(owner_code, Some(125), "inner cleanup must fail closed");
    assert!(started.elapsed() < CLEANUP_TIMEOUT + Duration::from_secs(1));
    assert_reaped(&owner, "fail-closed cleanup owner");
    assert_reaped(&leader, "post-snapshot admission leader");
    assert_reaped(&descendant, "post-snapshot admitted descendant");
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
    assert!(head.spawn_wall <= head.wall);
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
    assert!(tail.spawn_wall <= tail.wall);
    assert!(tail.stdout.is_empty());
    assert_eq!(tail.stderr, b"ERR_TAIL\n");

    let mut command = fixture_command("holding", directory.path());
    let deadline = Duration::from_millis(75);
    let timeout = run_bounded(&mut command, deadline, CapturePlan::discard()).unwrap();
    assert!(timeout.timed_out);
    assert!(timeout.spawn_wall <= timeout.wall);
    assert!(timeout.exit_code.is_none());
    assert!(timeout.status.is_none());
    assert!(timeout.stdout.is_empty() && timeout.stderr.is_empty());
    assert!(timeout.wall < deadline + Duration::from_secs(1));
}

/// Observe the published listener, prove it accepts, run `terminate`, then
/// prove the exact listener was reaped and its port closed.
#[cfg(any(windows, target_os = "linux"))]
fn assert_listener_closes(directory: &Path, terminate: impl FnOnce()) {
    let coordinates = await_file(&directory.join("listener"), READY_LIMIT);
    let listener = observe_published(&coordinates);
    let port: u16 = coordinates
        .split_whitespace()
        .nth(2)
        .and_then(|port| port.parse().ok())
        .expect("published listener port");
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
        "coordinates preceded listener readiness"
    );
    terminate();
    listener.reap(READY_LIMIT).expect("exact listener reaped");
    let deadline = Instant::now() + READY_LIMIT;
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(
            Instant::now() < deadline,
            "listener remained open after exact process exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn scope_cleanup_closes_descendant_listener() {
    crate::testing::contain_current_process().expect("contain source harness");
    let directory = tempfile::tempdir().unwrap();
    let mut command = fixture_command("tree-parent", directory.path());
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut parent = ProcessTree::spawn(&mut command).expect("own tree parent");
    assert_listener_closes(directory.path(), || {
        parent
            .terminate_and_reap()
            .expect("terminate and reap the complete child tree")
    });
}

#[test]
#[cfg(target_os = "linux")]
fn parent_death_signal_kills_a_leaf_child() {
    crate::testing::contain_current_process().expect("contain source harness");
    let directory = tempfile::tempdir().unwrap();
    let mut command = fixture_command("pdeath-supervisor", directory.path());
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut supervisor = ProcessTree::spawn(&mut command).expect("own pdeath supervisor");
    assert_listener_closes(directory.path(), || {
        supervisor
            .terminate_leader()
            .expect("terminate only the supervisor")
    });
    supervisor
        .terminate_and_reap()
        .expect("reap the parent-death supervisor scope");
}

/// Kills only the upstream owner on failure too, so the watched harness can
/// drain its nested scope before the outer scope is cleaned.
#[cfg(any(windows, target_os = "linux"))]
struct UpstreamOwner(ProcessTree, Option<ExactProcess>);

#[cfg(any(windows, target_os = "linux"))]
impl Drop for UpstreamOwner {
    fn drop(&mut self) {
        let _ = self.0.terminate_leader();
        if let Some(watched) = &self.1 {
            let _ = watched.reap(READY_LIMIT);
        }
        if let Err(error) = self.0.terminate_and_reap() {
            crate::abort_on_cleanup_failure("upstream owner scope did not drain", error);
        }
    }
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn upstream_owner_death_drains_watched_scopes() {
    crate::testing::contain_current_process().expect("contain source harness");
    let directory = tempfile::tempdir().unwrap();
    let mut command = fixture_command("supervisor", directory.path());
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut owner = UpstreamOwner(
        ProcessTree::spawn(&mut command).expect("own upstream supervisor"),
        None,
    );
    let watched = await_file(&directory.path().join("watched"), READY_LIMIT);
    owner.1 = Some(observe_published(&watched));
    await_file(&directory.path().join("watched-ready"), READY_LIMIT);
    assert_listener_closes(directory.path(), || {
        owner
            .0
            .terminate_leader()
            .expect("terminate only the owner")
    });
    owner
        .1
        .as_ref()
        .expect("retained watched harness")
        .reap(READY_LIMIT)
        .expect("watched harness exited and was reaped");
}
