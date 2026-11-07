//! Exact native process observation: a retained Linux pidfd or Windows
//! process HANDLE that a reused PID cannot retarget, plus the start
//! generation that ties a published PID to the retained object.

use std::io;
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

/// What a retained process object reports. Linux pidfd readiness separates
/// exit (a zombie) from reaping; Windows has no zombie state, so an exited
/// Windows process is `Reaped`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Exited,
    Reaped,
}

/// Decode pidfd poll readiness, failing closed on any other event bit.
#[cfg(target_os = "linux")]
pub fn decode_pidfd_events(events: libc::c_short) -> io::Result<ProcessState> {
    if events & !(libc::POLLIN | libc::POLLHUP) != 0 {
        return Err(io::Error::other(format!(
            "invalid pidfd poll events {events:#x}"
        )));
    }
    Ok(if events & libc::POLLHUP != 0 {
        ProcessState::Reaped
    } else if events & libc::POLLIN != 0 {
        ProcessState::Exited
    } else {
        ProcessState::Running
    })
}

/// A Linux process's start time in clock ticks since boot: field 22 of
/// `/proc/<pid>/stat`, counted after the final `)` because the command name
/// may itself contain spaces or parentheses.
#[cfg(target_os = "linux")]
pub fn start_ticks(pid: u32) -> io::Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    stat.rsplit_once(')')
        .and_then(|(_, tail)| tail.split_whitespace().nth(19))
        .and_then(|ticks| ticks.parse().ok())
        .ok_or_else(|| io::Error::other(format!("process {pid} start ticks unavailable")))
}

/// The rights a Windows HANDLE is opened with. A Linux pidfd always permits
/// both observation and signalling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Observe,
    Terminate,
}

#[cfg(target_os = "linux")]
pub struct ExactProcess {
    pid: u32,
    pidfd: std::os::fd::OwnedFd,
}

#[cfg(target_os = "linux")]
impl ExactProcess {
    /// Retain `pid`'s process object. Check its generation before trusting
    /// that the object is the process a coordinate named.
    pub fn open(pid: u32, _access: Access) -> io::Result<Self> {
        use std::os::fd::FromRawFd;
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0_u32) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            pid,
            pidfd: unsafe { std::os::fd::OwnedFd::from_raw_fd(raw as libc::c_int) },
        })
    }

    /// The start ticks, read by PID: bracket this with `state` checks so a
    /// replacement process's facts are never paired with this object.
    pub fn generation(&self) -> io::Result<u64> {
        start_ticks(self.pid)
    }

    /// Wait at most `timeout` for exit, then report the state. Interrupted
    /// polls resume until the timeout.
    pub fn state(&self, timeout: Duration) -> io::Result<ProcessState> {
        use std::os::fd::AsRawFd;
        let started = Instant::now();
        loop {
            let mut descriptor = libc::pollfd {
                fd: self.pidfd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let remaining = timeout.saturating_sub(started.elapsed());
            let result = unsafe {
                libc::poll(
                    &mut descriptor,
                    1,
                    remaining.as_millis().min(i32::MAX as u128) as i32,
                )
            };
            if result >= 0 {
                return decode_pidfd_events(descriptor.revents);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
            if started.elapsed() >= timeout {
                return Ok(ProcessState::Running);
            }
        }
    }

    /// Kill the retained process unless it has already exited.
    #[cfg(any(test, feature = "testing"))]
    pub fn terminate(&self) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        if self.state(Duration::ZERO)? != ProcessState::Running {
            return Ok(());
        }
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0_u32,
            )
        };
        let error = io::Error::last_os_error();
        if result != 0 && self.state(Duration::ZERO)? == ProcessState::Running {
            return Err(error);
        }
        Ok(())
    }

    /// Wait at most `timeout` until the process is reaped, reaping it here
    /// when it is this process's (possibly adopted) child. `P_PIDFD` cannot
    /// target a reused process ID the way a numeric wait could.
    #[cfg(any(test, feature = "testing"))]
    pub fn reap(&self, timeout: Duration) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        let deadline = Instant::now() + timeout;
        loop {
            if self.state(Duration::ZERO)? == ProcessState::Reaped {
                return Ok(());
            }
            let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PIDFD,
                    self.pidfd.as_raw_fd() as libc::id_t,
                    &mut status,
                    libc::WEXITED | libc::WNOHANG,
                )
            };
            if result == 0 && unsafe { status.si_pid() } != 0 {
                return Ok(());
            }
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ECHILD)
                    && error.kind() != io::ErrorKind::Interrupted
                {
                    return Err(error);
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "exact process exited but was not proved reaped",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(windows)]
pub struct ExactProcess(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl ExactProcess {
    /// Retain `pid`'s process object. Check its generation before trusting
    /// that the object is the process a coordinate named.
    pub fn open(pid: u32, access: Access) -> io::Result<Self> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        };
        let mut rights = PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION;
        if access == Access::Terminate {
            rights |= PROCESS_TERMINATE;
        }
        let handle = unsafe { OpenProcess(rights, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    /// The creation time as a FILETIME, read through the retained HANDLE.
    pub fn generation(&self) -> io::Result<u64> {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::GetProcessTimes;
        let [mut created, mut exited, mut kernel, mut user] = [FILETIME::default(); 4];
        if unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    /// Wait at most `timeout` (never infinitely) for exit, then report the
    /// state.
    pub fn state(&self, timeout: Duration) -> io::Result<ProcessState> {
        use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let milliseconds = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        match unsafe { WaitForSingleObject(self.0, milliseconds) } {
            WAIT_OBJECT_0 => Ok(ProcessState::Reaped),
            WAIT_TIMEOUT => Ok(ProcessState::Running),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            state => Err(io::Error::other(format!(
                "unexpected exact process wait state {state}"
            ))),
        }
    }

    /// Kill the retained process unless it has already exited.
    #[cfg(any(test, feature = "testing"))]
    pub fn terminate(&self) -> io::Result<()> {
        use windows_sys::Win32::System::Threading::TerminateProcess;
        if self.state(Duration::ZERO)? != ProcessState::Running {
            return Ok(());
        }
        let terminated = unsafe { TerminateProcess(self.0, 1) };
        let error = io::Error::last_os_error();
        if terminated == 0 && self.state(Duration::ZERO)? == ProcessState::Running {
            return Err(error);
        }
        Ok(())
    }

    /// Wait at most `timeout` for exit: Windows has no reaping obligation.
    #[cfg(any(test, feature = "testing"))]
    pub fn reap(&self, timeout: Duration) -> io::Result<()> {
        match self.state(timeout)? {
            ProcessState::Running => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "exact process did not exit before its deadline",
            )),
            _ => Ok(()),
        }
    }

    /// The exit code of the exited process.
    #[cfg(any(test, feature = "testing"))]
    pub fn exit_code(&self) -> io::Result<u32> {
        let mut code = 0;
        if unsafe { windows_sys::Win32::System::Threading::GetExitCodeProcess(self.0, &mut code) }
            == 0
        {
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

#[cfg(all(test, target_os = "linux"))]
#[test]
fn pidfd_event_decoder_distinguishes_exit_reaping_and_invalid() {
    assert_eq!(decode_pidfd_events(0).unwrap(), ProcessState::Running);
    assert_eq!(
        decode_pidfd_events(libc::POLLIN).unwrap(),
        ProcessState::Exited
    );
    assert_eq!(
        decode_pidfd_events(libc::POLLHUP).unwrap(),
        ProcessState::Reaped
    );
    assert_eq!(
        decode_pidfd_events(libc::POLLIN | libc::POLLHUP).unwrap(),
        ProcessState::Reaped
    );
    for invalid in [
        libc::POLLERR,
        libc::POLLNVAL,
        libc::POLLERR | libc::POLLIN,
        libc::POLLIN | libc::POLLNVAL,
    ] {
        assert!(decode_pidfd_events(invalid).is_err());
    }
}
