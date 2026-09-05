//! Bounded ownership of native subprocess scopes.
//!
//! Windows children start suspended, enter a kill-on-close Job, and only then
//! resume. Linux/macOS/FreeBSD children enter a cooperative process group: this is not a
//! security boundary against `setsid`, group escape, or abrupt owner SIGKILL.
//! Normal return and unwind clean the complete owned scope. Controlled Linux
//! orphan tests must explicitly enable a scoped subreaper or use a reaping PID
//! namespace. Neither spawning nor capture silently installs process-wide
//! parent watchers or changes subreaper policy. Other Unix targets fail closed
//! until a non-reaping identity adapter is implemented. Native acceptance here
//! covers Windows/Linux; macOS/FreeBSD retain their POSIX path without a parity
//! claim or implicit subreaper/parent-watcher support.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
#[path = "unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(any(unix, test))]
mod registry;
mod supervision;

#[cfg(target_os = "linux")]
pub use supervision::{LinuxProcessState, decode_pidfd_events};
pub use supervision::{enable_subreaper, watch_current_parent};

pub const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Owns a Windows Job or cooperative Unix process group until cleanup succeeds.
pub struct ProcessTree {
    scope: Option<platform::ChildScope>,
}

impl ProcessTree {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        platform::ChildScope::spawn(command).map(|scope| Self { scope: Some(scope) })
    }

    pub fn child(&self) -> &Child {
        self.scope
            .as_ref()
            .expect("process scope remains owned")
            .child()
    }

    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.scope
            .as_mut()
            .expect("owned scope")
            .child_mut()
            .stdin
            .take()
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.scope
            .as_mut()
            .expect("owned scope")
            .child_mut()
            .stdout
            .take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.scope
            .as_mut()
            .expect("owned scope")
            .child_mut()
            .stderr
            .take()
    }

    /// Signal the exact direct child without reaping it. Source regressions
    /// use this to test owner-death behavior before draining its owned scope.
    pub fn terminate_leader(&mut self) -> io::Result<()> {
        self.scope.as_mut().expect("owned scope").terminate_leader()
    }

    pub fn try_wait_leader(&mut self) -> io::Result<Option<ExitStatus>> {
        self.scope
            .as_mut()
            .expect("process scope remains owned")
            .try_wait_leader()
    }

    /// Observe the leader, then clean descendants before returning its status.
    /// Timeout and observation failures also complete checked scope cleanup.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        let started = Instant::now();
        let observed = loop {
            match self.try_wait_leader() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if started.elapsed() >= timeout => {
                    break Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "child execution deadline exceeded",
                    ));
                }
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
                Err(error) => break Err(error),
            }
        };
        self.terminate_and_reap()?;
        observed
    }

    /// Terminate the scope and prove it drained within a separate five-second
    /// cleanup deadline. Unprovable cleanup exits the owner with code 125 rather than return
    /// an apparently reusable process boundary with live children.
    pub fn terminate_and_reap(&mut self) -> io::Result<()> {
        if let Some(mut scope) = self.scope.take() {
            scope.cleanup()?;
        }
        Ok(())
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Err(error) = self.terminate_and_reap() {
            abort_on_cleanup_failure("owned process scope cleanup failed", error);
        }
    }
}

/// Fail closed after attempting cleanup of registered Unix scopes. This is a
/// last-resort failure path, never successful acceptance evidence. Exit code
/// 125 avoids crash dialogs/core dumps while still closing native Job handles.
pub fn abort_on_cleanup_failure(context: &str, error: impl std::fmt::Display) -> ! {
    eprintln!("{context}: {error}");
    #[cfg(unix)]
    platform::shutdown_owned_scopes();
    std::process::exit(125)
}

#[derive(Clone, Copy)]
pub enum CaptureMode {
    Head,
    Tail,
}

#[derive(Clone, Copy)]
pub struct CapturePlan {
    stdout_limit: usize,
    stdout_mode: CaptureMode,
    stderr_limit: usize,
    stderr_mode: CaptureMode,
}

impl CapturePlan {
    pub const fn discard() -> Self {
        Self::head(0, 0)
    }
    pub const fn stderr_tail(limit: usize) -> Self {
        Self {
            stdout_limit: 0,
            stdout_mode: CaptureMode::Head,
            stderr_limit: limit,
            stderr_mode: CaptureMode::Tail,
        }
    }
    pub const fn head(stdout_limit: usize, stderr_limit: usize) -> Self {
        Self {
            stdout_limit,
            stdout_mode: CaptureMode::Head,
            stderr_limit,
            stderr_mode: CaptureMode::Head,
        }
    }
}

pub struct ProcessOutcome {
    pub exit_code: Option<i32>,
    pub status: Option<ExitStatus>,
    pub timed_out: bool,
    pub wall: Duration,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Capture through temporary files, avoiding pipe EOF dependencies on inherited
/// writers. Limits bound retained memory, not disk generation; this API is not
/// a disk-quota sandbox for hostile commands.
pub fn run_bounded(
    command: &mut Command,
    timeout: Duration,
    capture: CapturePlan,
) -> io::Result<ProcessOutcome> {
    let mut stdout = CaptureFile::new(capture.stdout_limit)?;
    let mut stderr = CaptureFile::new(capture.stderr_limit)?;
    command.stdout(stdout.stdio()?).stderr(stderr.stdio()?);
    let started = Instant::now();
    let mut tree = ProcessTree::spawn(command)?;
    let (status, timed_out) = loop {
        match tree.try_wait_leader() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None) if started.elapsed() >= timeout => break (None, true),
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(error) => {
                tree.terminate_and_reap()?;
                return Err(error);
            }
        }
    };
    let wall = started.elapsed();
    tree.terminate_and_reap()?;
    Ok(ProcessOutcome {
        exit_code: status.and_then(|status| status.code()),
        status,
        timed_out,
        wall,
        stdout: stdout.read(capture.stdout_mode)?,
        stderr: stderr.read(capture.stderr_mode)?,
    })
}

struct CaptureFile {
    file: Option<File>,
    limit: usize,
}

impl CaptureFile {
    fn new(limit: usize) -> io::Result<Self> {
        Ok(Self {
            file: (limit > 0).then(tempfile::tempfile).transpose()?,
            limit,
        })
    }
    fn stdio(&self) -> io::Result<Stdio> {
        self.file.as_ref().map_or_else(
            || Ok(Stdio::null()),
            |file| file.try_clone().map(Stdio::from),
        )
    }
    fn read(&mut self, mode: CaptureMode) -> io::Result<Vec<u8>> {
        let Some(file) = &mut self.file else {
            return Ok(Vec::new());
        };
        let length = file.metadata()?.len();
        let limit = self.limit as u64;
        file.seek(SeekFrom::Start(match mode {
            CaptureMode::Head => 0,
            CaptureMode::Tail => length.saturating_sub(limit),
        }))?;
        let mut output = Vec::with_capacity(length.min(limit) as usize);
        file.take(limit).read_to_end(&mut output)?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
