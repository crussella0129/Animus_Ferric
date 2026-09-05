use std::io;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus};
use std::time::Instant;

use crate::{CLEANUP_TIMEOUT, POLL_INTERVAL, registry};

pub(crate) struct ChildScope {
    child: Option<Child>,
    pgid: libc::pid_t,
    key: registry::ScopeKey,
}

impl ChildScope {
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
        {
            let _ = command;
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "non-reaping identity observation is implemented for Linux, macOS and FreeBSD",
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
        {
            // Shutdown cannot miss a child between native spawn and registration.
            let mut registry = registry::lock();
            registry.allow_spawn()?;
            let child = command.process_group(0).spawn()?;
            let pgid = child.id() as libc::pid_t;
            let key = registry.register(child.id());
            Ok(Self {
                child: Some(child),
                pgid,
                key,
            })
        }
    }

    pub(crate) fn child(&self) -> &Child {
        self.child.as_ref().expect("scope remains armed")
    }
    pub(crate) fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("scope remains armed")
    }

    pub(crate) fn terminate_leader(&mut self) -> io::Result<()> {
        let registry = registry::lock();
        registry.allow_spawn()?;
        if !registry.contains(self.key) {
            return Err(io::Error::other("owned scope already drained"));
        }
        require_unreaped_leader(self.pgid)?;
        // Same lock as watcher reaping/removal. The direct leader remains an
        // unreaped anchor for a later whole-group signal; do not mark that
        // whole-group signal as already sent merely for this owner-death seam.
        self.child_mut().kill()
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    pub(crate) fn try_wait_leader(&mut self) -> io::Result<Option<ExitStatus>> {
        use std::os::unix::process::ExitStatusExt;
        let registry = registry::lock();
        registry.allow_spawn()?;
        if !registry.contains(self.key) {
            return Err(io::Error::other("owned scope already drained"));
        }
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.pgid as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(error);
        }
        let info = unsafe { info.assume_init() };
        if unsafe { info.si_pid() } == 0 {
            return Ok(None);
        }
        let status = unsafe { info.si_status() };
        let raw = match info.si_code {
            libc::CLD_EXITED => status << 8,
            libc::CLD_KILLED => status,
            libc::CLD_DUMPED => status | 0x80,
            other => {
                return Err(io::Error::other(format!(
                    "unexpected child wait event {other}"
                )));
            }
        };
        Ok(Some(ExitStatus::from_raw(raw)))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
    pub(crate) fn try_wait_leader(&mut self) -> io::Result<Option<ExitStatus>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "non-reaping observation unavailable",
        ))
    }

    pub(crate) fn cleanup(&mut self) -> io::Result<()> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        let mut leader_reaped = false;
        let mut diagnostics = Vec::new();
        loop {
            let mut registry = registry::lock();
            if !registry.contains(self.key) {
                // Shutdown already proved this scope absent while holding the
                // same lock. Never touch its potentially reusable number again.
                self.child.take();
                return Ok(());
            }
            if let Err(error) = registry.signal_once(self.key, || signal_anchored_group(self.pgid))
            {
                drop(registry);
                crate::abort_on_cleanup_failure(
                    "lost anchored process-group termination authority",
                    error,
                );
            }
            if !leader_reaped {
                match child.try_wait() {
                    Ok(Some(_)) => leader_reaped = true,
                    Ok(None) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
                        leader_reaped = true
                    }
                    Err(error) => diagnostics.push(error.to_string()),
                }
            }
            if leader_reaped {
                // If this process opted into subreaping, adopted descendants
                // are ours to reap. ECHILD is normal without adopted children.
                if let Err(error) = reap_group(self.pgid) {
                    diagnostics.push(error.to_string());
                }
                match registry.remove_if_absent(self.key, || group_absent(self.pgid)) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(error) => diagnostics.push(error.to_string()),
                }
            }
            drop(registry);
            if Instant::now() >= deadline {
                crate::abort_on_cleanup_failure(
                    "Unix scope did not drain in five seconds",
                    diagnostics.join("; "),
                );
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        self.child.take();
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(diagnostics.join("; ")))
        }
    }
}

impl Drop for ChildScope {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            crate::abort_on_cleanup_failure("Unix scope drop failed", error);
        }
    }
}

fn signal_anchored_group(pgid: libc::pid_t) -> io::Result<()> {
    require_unreaped_leader(pgid)?;
    if unsafe { libc::kill(-pgid, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

fn require_unreaped_leader(pgid: libc::pid_t) -> io::Result<()> {
    // The retained, unreaped leader prevents the numeric PGID from being
    // reused. Check before the sole signal; never signal again after reaping.
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        loop {
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    pgid as libc::id_t,
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            } == 0
            {
                break;
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ECHILD) {
                // An exact fixture reaper may already have completed ownership.
                // No remaining numeric process group is destructive authority.
                return Err(io::Error::other(
                    "process-group leader was reaped outside its retained scope",
                ));
            }
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
    {
        let _ = pgid;
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "non-reaping identity observation unavailable",
        ));
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    Ok(())
}

fn group_absent(pgid: libc::pid_t) -> io::Result<bool> {
    if unsafe { libc::kill(-pgid, 0) } == 0 {
        return Ok(false);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(true),
        Some(libc::EPERM) => Ok(false),
        _ => Err(error),
    }
}

fn reap_group(pgid: libc::pid_t) -> io::Result<()> {
    loop {
        let result = unsafe { libc::waitpid(-pgid, std::ptr::null_mut(), libc::WNOHANG) };
        if result > 0 {
            continue;
        }
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ECHILD) => return Ok(()),
            Some(libc::EINTR) => continue,
            _ => return Err(error),
        }
    }
}

pub(crate) fn shutdown_owned_scopes() {
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    loop {
        let mut registry = registry::lock();
        registry.begin_shutdown();
        let keys = registry.keys();
        // The lock stays held while these numbers are signalled and observed;
        // there is no stale snapshot surviving normal final removal.
        for key in keys {
            if registry
                .signal_once(key, || signal_anchored_group(key.id as libc::pid_t))
                .is_ok()
            {
                let _ = reap_group(key.id as libc::pid_t);
                let _ = registry.remove_if_absent(key, || group_absent(key.id as libc::pid_t));
            }
        }
        if registry.is_empty() || Instant::now() >= deadline {
            return;
        }
        drop(registry);
        std::thread::sleep(POLL_INTERVAL);
    }
}
