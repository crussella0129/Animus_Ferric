use std::io;

/// Explicit process-wide opt-in for controlled Linux test hosts. This changes
/// adoption policy only: cleanup still waits solely on owned process groups or
/// exact fixture children. No background waitpid(-1) reaper is installed.
pub fn enable_subreaper() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Explicit process-lifetime supervision for source test harnesses. Normal
/// `ProcessTree::spawn` and benchmark calls never install this policy implicitly.
/// A Linux watcher must remain alive to sweep scopes; an external reaping
/// supervisor/namespace is required when forcibly killing that watcher owner.
pub fn watch_current_parent() -> io::Result<()> {
    #[cfg(windows)]
    return crate::platform::watch_current_parent().map_err(io::Error::other);
    #[cfg(target_os = "linux")]
    {
        static WATCH: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        WATCH
            .get_or_init(linux::install_parent_watcher)
            .clone()
            .map_err(io::Error::other)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxProcessState {
    Running,
    Exited,
    Reaped,
}

/// Linux pidfd readiness distinguishes exit (including zombies) from reaping.
#[cfg(target_os = "linux")]
pub fn decode_pidfd_events(events: libc::c_short) -> io::Result<LinuxProcessState> {
    if events & !(libc::POLLIN | libc::POLLHUP) != 0 {
        return Err(io::Error::other(format!(
            "invalid pidfd poll events {events:#x}"
        )));
    }
    Ok(if events & libc::POLLHUP != 0 {
        LinuxProcessState::Reaped
    } else if events & libc::POLLIN != 0 {
        LinuxProcessState::Exited
    } else {
        LinuxProcessState::Running
    })
}

#[cfg(target_os = "linux")]
pub(crate) mod linux {
    use super::{LinuxProcessState, decode_pidfd_events};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::time::{Duration, Instant};

    pub(crate) struct ExactParent {
        descriptor: OwnedFd,
    }

    impl ExactParent {
        pub(crate) fn acquire() -> io::Result<Self> {
            let parent = unsafe { libc::getppid() };
            if parent <= 1 {
                return Err(io::Error::other(
                    "no independently observable launching parent",
                ));
            }
            let generation = start_ticks(parent)?;
            let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, parent, 0_u32) };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let exact = Self {
                descriptor: unsafe { OwnedFd::from_raw_fd(raw as libc::c_int) },
            };
            if unsafe { libc::getppid() } != parent
                || start_ticks(parent)? != generation
                || exact.poll(Duration::ZERO)? != LinuxProcessState::Running
            {
                return Err(io::Error::other(
                    "launching parent changed during watcher setup",
                ));
            }
            Ok(exact)
        }

        pub(crate) fn poll(&self, timeout: Duration) -> io::Result<LinuxProcessState> {
            let started = Instant::now();
            loop {
                let mut descriptor = libc::pollfd {
                    fd: self.descriptor.as_raw_fd(),
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
                    return Ok(LinuxProcessState::Running);
                }
            }
        }
    }

    fn start_ticks(pid: libc::pid_t) -> io::Result<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        stat.rsplit_once(')')
            .and_then(|(_, tail)| tail.split_whitespace().nth(19))
            .and_then(|ticks| ticks.parse().ok())
            .ok_or_else(|| io::Error::other("parent start ticks unavailable"))
    }

    pub(crate) fn install_parent_watcher() -> Result<(), String> {
        let parent = ExactParent::acquire().map_err(|error| error.to_string())?;
        std::thread::Builder::new()
            .name("ferric-parent-watch".into())
            .spawn(move || {
                // Calling a method captures the owner, not a copied raw fd field.
                // OwnedFd remains alive throughout every poll in this thread.
                loop {
                    match parent.poll(Duration::from_millis(100)) {
                        Ok(LinuxProcessState::Running) => {}
                        Ok(_) | Err(_) => {
                            crate::platform::shutdown_owned_scopes();
                            std::process::exit(125);
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
#[test]
fn pidfd_event_decoder_distinguishes_exit_reaping_and_invalid() {
    assert_eq!(decode_pidfd_events(0).unwrap(), LinuxProcessState::Running);
    assert_eq!(
        decode_pidfd_events(libc::POLLIN).unwrap(),
        LinuxProcessState::Exited
    );
    assert_eq!(
        decode_pidfd_events(libc::POLLIN | libc::POLLHUP).unwrap(),
        LinuxProcessState::Reaped
    );
    assert!(decode_pidfd_events(libc::POLLNVAL).is_err());
    assert!(decode_pidfd_events(libc::POLLERR | libc::POLLIN).is_err());
}

#[cfg(all(test, target_os = "linux"))]
#[test]
fn parent_watch_retains_identity() {
    let parent = linux::ExactParent::acquire().unwrap();
    let observer = std::thread::spawn(move || {
        for _ in 0..3 {
            // File allocation would readily reuse a prematurely closed raw fd.
            let _files: Vec<_> = (0..16).map(|_| tempfile::tempfile().unwrap()).collect();
            assert_eq!(
                parent.poll(std::time::Duration::from_millis(20)).unwrap(),
                LinuxProcessState::Running
            );
        }
    });
    observer.join().unwrap();
}
