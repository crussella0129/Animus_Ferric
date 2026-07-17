use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use tokio::process::Child;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Running,
    Completed { exit_code: i32 },
    Terminated,
    Error(String),
}

pub struct BackgroundTask {
    pub id: String,
    pub command: String,
    pub log_path: PathBuf,
    pub child: Mutex<Child>,
    pub status: RwLock<TaskStatus>,
}

type Registry = RwLock<HashMap<String, Arc<BackgroundTask>>>;

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn get_registry() -> &'static Registry {
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
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

    get_registry().write().unwrap().insert(id, task.clone());
    task
}

pub fn get_task(id: &str) -> Option<Arc<BackgroundTask>> {
    get_registry().read().unwrap().get(id).cloned()
}

pub fn list_tasks() -> Vec<Arc<BackgroundTask>> {
    get_registry().read().unwrap().values().cloned().collect()
}
