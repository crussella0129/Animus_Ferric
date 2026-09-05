//! Thin source-test adapter over the shared native subprocess boundary.
#![allow(dead_code)]

use std::io;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::Duration;

pub(crate) fn ensure_current_process_tree_is_contained() -> Result<(), String> {
    ferric_process::enable_subreaper().map_err(|error| error.to_string())?;
    ferric_process::watch_current_parent().map_err(|error| error.to_string())
}

/// Compatibility name for source helpers. A watcher-owning process must live
/// long enough to sweep its scopes, so this does not arm immediate SIGKILL.
pub(crate) fn arm_current_process_parent_death_signal() -> Result<(), String> {
    ensure_current_process_tree_is_contained()
}

/// Explicit leaf-only parent-death policy for deliberate native regressions.
/// Never apply this to a process whose watcher must clean separate scopes.
pub(crate) fn configure_command_parent_death_signal(command: &mut Command) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let expected_parent = std::process::id() as libc::pid_t;
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != expected_parent {
                    return Err(io::Error::from_raw_os_error(libc::ESRCH));
                }
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = command;
    Ok(())
}

pub(crate) fn abort_on_cleanup_failure(context: &str, error: impl std::fmt::Display) -> ! {
    ferric_process::abort_on_cleanup_failure(context, error)
}

pub(crate) struct ContainedChild(ferric_process::ProcessTree);

impl ContainedChild {
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        ensure_current_process_tree_is_contained().map_err(io::Error::other)?;
        ferric_process::ProcessTree::spawn(command).map(Self)
    }
    pub(crate) fn child(&self) -> &Child {
        self.0.child()
    }
    pub(crate) fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.0.take_stdin()
    }
    pub(crate) fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.0.take_stdout()
    }
    pub(crate) fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.0.take_stderr()
    }
    pub(crate) fn terminate_leader(&mut self) -> io::Result<()> {
        self.0.terminate_leader()
    }
    pub(crate) fn try_wait_leader(&mut self) -> io::Result<Option<ExitStatus>> {
        self.0.try_wait_leader()
    }
    pub(crate) fn wait_for_exit_and_disarm(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        self.0.wait_for_exit(timeout)
    }
    pub(crate) fn terminate_and_reap(&mut self) -> io::Result<()> {
        self.0.terminate_and_reap()
    }
}

fn output_bounded_with_stdin(
    command: &mut Command,
    stdin: Stdio,
    timeout: Duration,
) -> io::Result<Output> {
    ensure_current_process_tree_is_contained().map_err(io::Error::other)?;
    command.stdin(stdin);
    // Test diagnostics are bounded independently of the benchmark's smaller
    // head/tail limits; exceeding this cap cannot allocate unbounded memory.
    let result = ferric_process::run_bounded(
        command,
        timeout,
        ferric_process::CapturePlan::head(16 * 1024 * 1024, 16 * 1024 * 1024),
    )?;
    let status = result.status.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("source test child exceeded {timeout:?} after checked cleanup"),
        )
    })?;
    Ok(Output {
        status,
        stdout: result.stdout,
        stderr: result.stderr,
    })
}

pub(crate) fn output_bounded(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    output_bounded_with_stdin(command, Stdio::null(), timeout)
}

pub(crate) fn output_bounded_with_input(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
) -> io::Result<Output> {
    use std::io::{Seek, SeekFrom, Write};
    let mut stdin = tempfile::tempfile()?;
    stdin.write_all(input)?;
    stdin.seek(SeekFrom::Start(0))?;
    output_bounded_with_stdin(command, Stdio::from(stdin), timeout)
}
