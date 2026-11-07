//! Source-test containment for this crate's tests and, through the `testing`
//! feature, for dependent crates' tests. Each harness that spawns children
//! contains itself first, so an abandoned harness still sweeps its scopes.

use std::io;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use crate::{CapturePlan, ProcessTree, enable_subreaper, run_bounded, watch_current_parent};

/// Opt into subreaping, then watch the exact launching parent. A watcher-owning
/// process must live long enough to sweep its scopes, so this does not arm an
/// immediate parent-death SIGKILL.
pub fn contain_current_process() -> io::Result<()> {
    enable_subreaper()?;
    watch_current_parent()
}

/// Contain this harness, then spawn `command` in an owned process scope.
pub fn spawn(command: &mut Command) -> io::Result<ProcessTree> {
    contain_current_process()?;
    ProcessTree::spawn(command)
}

/// Explicit leaf-only Linux parent-death policy for deliberate native
/// regressions. Never apply this to a process whose watcher must clean
/// separate scopes. Other platforms leave `command` unchanged.
pub fn parent_death_signal(command: &mut Command) {
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
}

/// Re-run exactly `test_name`, alone, in the running test binary.
pub fn harness_command(test_name: &str) -> Command {
    let mut command =
        Command::new(std::env::current_exe().expect("locate the running test harness"));
    command.args(["--exact", test_name, "--nocapture", "--test-threads=1"]);
    command
}

/// Run `command` to completion within `timeout`, with its stdin closed; every
/// owned child is reaped first. Output is bounded at 16 MiB per stream.
pub fn output(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    output_with_stdin(command, Stdio::null(), timeout)
}

/// [`output`], with `input` as the child's stdin.
pub fn output_with_input(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
) -> io::Result<Output> {
    use std::io::{Seek, SeekFrom, Write};
    let mut stdin = tempfile::tempfile()?;
    stdin.write_all(input)?;
    stdin.seek(SeekFrom::Start(0))?;
    output_with_stdin(command, Stdio::from(stdin), timeout)
}

fn output_with_stdin(command: &mut Command, stdin: Stdio, timeout: Duration) -> io::Result<Output> {
    contain_current_process()?;
    command.stdin(stdin);
    // Test diagnostics are bounded independently of the benchmark's smaller
    // head/tail limits; exceeding this cap cannot allocate unbounded memory.
    let result = run_bounded(
        command,
        timeout,
        CapturePlan::head(16 * 1024 * 1024, 16 * 1024 * 1024),
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
