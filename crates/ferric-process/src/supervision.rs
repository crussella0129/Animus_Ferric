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
pub(crate) mod linux {
    use crate::exact::{Access, ExactProcess, ProcessState, start_ticks};
    use std::io;
    use std::time::Duration;

    /// Retain the launching parent's exact process object, refusing a parent
    /// that changed (or exited) while it was being acquired.
    pub(crate) fn exact_parent() -> io::Result<ExactProcess> {
        let parent = unsafe { libc::getppid() };
        if parent <= 1 {
            return Err(io::Error::other(
                "no independently observable launching parent",
            ));
        }
        let generation = start_ticks(parent as u32)?;
        let exact = ExactProcess::open(parent as u32, Access::Observe)?;
        if unsafe { libc::getppid() } != parent
            || exact.generation()? != generation
            || exact.state(Duration::ZERO)? != ProcessState::Running
        {
            return Err(io::Error::other(
                "launching parent changed during watcher setup",
            ));
        }
        Ok(exact)
    }

    pub(crate) fn install_parent_watcher() -> Result<(), String> {
        let parent = exact_parent().map_err(|error| error.to_string())?;
        std::thread::Builder::new()
            .name("ferric-parent-watch".into())
            .spawn(move || {
                // Calling a method captures the owner, not a copied raw fd field.
                // OwnedFd remains alive throughout every poll in this thread.
                loop {
                    match parent.state(Duration::from_millis(100)) {
                        Ok(ProcessState::Running) => {}
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
fn parent_watch_retains_identity() {
    use crate::exact::ProcessState;
    let parent = linux::exact_parent().unwrap();
    let observer = std::thread::spawn(move || {
        for _ in 0..3 {
            // File allocation would readily reuse a prematurely closed raw fd.
            let _files: Vec<_> = (0..16).map(|_| tempfile::tempfile().unwrap()).collect();
            assert_eq!(
                parent.state(std::time::Duration::from_millis(20)).unwrap(),
                ProcessState::Running
            );
        }
    });
    observer.join().unwrap();
}
