//! Process-global registry of detached background tasks (`shell_exec
//! --background`), plus the `manage_task` tool's view of them.
//!
//! **No lock acquisition here may panic.** `manage_task` is human-invokable, and
//! `Tool::run` is called from the loop's dispatch chokepoint — a panic there
//! takes down the whole harness, not just the call. A `std` lock poisons when
//! *any* holder panics, so `.unwrap()` on a lock turns one unlucky task thread
//! into "every subsequent `manage_task` call aborts the process" (ADR-074).
//!
//! The guarded data is a status enum and a `Child` handle; neither has an
//! invariant a panicking writer could leave half-broken. Recovering the guard
//! with `into_inner()` is therefore both safe and strictly better than aborting.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::process::Child;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Running,
    Completed { exit_code: i32 },
    Terminated,
    Error(String),
}

impl TaskStatus {
    /// The single rendering of a status. Was duplicated character-for-character
    /// between `manage_task`'s `list` and `status` arms.
    pub fn label(&self) -> String {
        match self {
            TaskStatus::Running => "Running".to_string(),
            TaskStatus::Completed { exit_code } => format!("Completed (Exit {exit_code})"),
            TaskStatus::Terminated => "Terminated".to_string(),
            TaskStatus::Error(e) => format!("Error: {e}"),
        }
    }
}

pub struct BackgroundTask {
    pub id: String,
    pub command: String,
    pub log_path: PathBuf,
    pub child: Mutex<Child>,
    pub status: RwLock<TaskStatus>,
}

impl BackgroundTask {
    /// Lock the child handle, recovering from a poisoned mutex rather than
    /// panicking. See the module note.
    pub fn child(&self) -> MutexGuard<'_, Child> {
        self.child.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status_read(&self) -> RwLockReadGuard<'_, TaskStatus> {
        self.status.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status_write(&self) -> RwLockWriteGuard<'_, TaskStatus> {
        self.status.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Poll a still-`Running` task and record its exit. `try_wait` also reaps
    /// the OS process, so this is what keeps finished children from lingering
    /// as zombies.
    pub fn poll_status(&self) {
        let mut status = self.status_write();
        if *status != TaskStatus::Running {
            return;
        }
        if let Ok(Some(exit)) = self.child().try_wait() {
            *status = TaskStatus::Completed {
                exit_code: exit.code().unwrap_or(-1),
            };
        }
    }
}

type Registry = RwLock<HashMap<String, Arc<BackgroundTask>>>;

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn get_registry() -> &'static Registry {
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

fn registry_read() -> RwLockReadGuard<'static, HashMap<String, Arc<BackgroundTask>>> {
    get_registry().read().unwrap_or_else(|e| e.into_inner())
}

fn registry_write() -> RwLockWriteGuard<'static, HashMap<String, Arc<BackgroundTask>>> {
    get_registry().write().unwrap_or_else(|e| e.into_inner())
}

pub fn spawn_task(
    id: String,
    command: String,
    log_path: PathBuf,
    child: Child,
) -> Arc<BackgroundTask> {
    let task = Arc::new(BackgroundTask {
        id: id.clone(),
        command,
        log_path,
        child: Mutex::new(child),
        status: RwLock::new(TaskStatus::Running),
    });

    registry_write().insert(id, task.clone());
    task
}

pub fn get_task(id: &str) -> Option<Arc<BackgroundTask>> {
    registry_read().get(id).cloned()
}

pub fn list_tasks() -> Vec<Arc<BackgroundTask>> {
    registry_read().values().cloned().collect()
}

/// Drop a task from the registry, returning it if it was there.
///
/// Finished tasks are deliberately *retained* until dropped explicitly — their
/// recorded status and log path are the only evidence the run happened, so
/// auto-removing them on completion would make `manage_task status` racy. This
/// is the removal path that was missing entirely (C4): without it the map, and
/// the `Child` handles it owns, grew for the lifetime of the process.
pub fn remove_task(id: &str) -> Option<Arc<BackgroundTask>> {
    registry_write().remove(id)
}

/// Drop every task that is no longer running, returning how many went. Polls
/// first so a task that exited since the last look is counted as finished.
pub fn remove_finished_tasks() -> usize {
    for task in list_tasks() {
        task.poll_status();
    }
    let mut reg = registry_write();
    let before = reg.len();
    reg.retain(|_, t| *t.status_read() == TaskStatus::Running);
    before - reg.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A poisoned lock must not take the process with it — this is the whole
    /// point of the module (ADR-074). Under the old `.unwrap()` code every
    /// accessor after this panic aborted the harness.
    #[test]
    fn a_poisoned_status_lock_is_recovered_not_fatal() {
        let lock = RwLock::new(TaskStatus::Running);

        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock.write().unwrap();
            panic!("holder panicked while writing");
        }));
        assert!(poisoned.is_err());
        assert!(lock.is_poisoned(), "precondition: the lock is now poisoned");

        // The recovery pattern the accessors use.
        let recovered = lock.read().unwrap_or_else(|e| e.into_inner());
        assert_eq!(*recovered, TaskStatus::Running);
    }

    #[test]
    fn status_label_renders_every_variant() {
        assert_eq!(TaskStatus::Running.label(), "Running");
        assert_eq!(
            TaskStatus::Completed { exit_code: 3 }.label(),
            "Completed (Exit 3)"
        );
        assert_eq!(TaskStatus::Terminated.label(), "Terminated");
        assert_eq!(
            TaskStatus::Error("boom".into()).label(),
            "Error: boom".to_string()
        );
    }
}
