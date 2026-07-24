use serde_json::json;
use std::fs;
use std::io::{Seek, SeekFrom};

use super::task_registry::{TaskStatus, get_task, list_tasks, remove_finished_tasks, remove_task};
use crate::spec::{Tool, ToolCtx, ToolSpec};
use ferric_guard::PermissionLevel;

/// Bytes of log tail returned by `status`.
const LOG_TAIL_BYTES: u64 = 5000;

pub struct ManageTask;

impl Tool for ManageTask {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "manage_task".to_string(),
            description: "Manage background tasks. Actions: 'list' (all tasks), 'status' (view logs and status of a specific task), 'kill' (terminate a task), 'send_input' (send stdin to a task), 'remove' (drop a finished task from the list; omit task_id to drop all finished).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "status", "kill", "send_input", "remove"] },
                    "task_id": { "type": "string" },
                    "input": { "type": "string", "description": "Input string to send to stdin" }
                },
                "required": ["action"]
            }),
            permission: PermissionLevel::Execute,
            ring: 2,
        }
    }

    fn run(&self, _ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or("missing 'action' argument")?;

        match action {
            "list" => Ok(render_list()),
            "status" => render_status(task_id(args)?),
            "kill" => kill(task_id(args)?),
            "send_input" => {
                let input = args
                    .get("input")
                    .and_then(|v| v.as_str())
                    .ok_or("missing 'input'")?;
                send_input(task_id(args)?, input)
            }
            "remove" => Ok(remove(args.get("task_id").and_then(|v| v.as_str()))),
            _ => Err(format!("Unknown action: {action}")),
        }
    }
}

fn task_id(args: &serde_json::Value) -> Result<&str, String> {
    args.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'task_id'".to_string())
}

fn render_list() -> String {
    let tasks = list_tasks();
    if tasks.is_empty() {
        return "No background tasks running or tracked.".to_string();
    }

    let mut out = String::from("| ID | Status | Command |\n|---|---|---|\n");
    for t in tasks {
        t.poll_status();
        let label = t.status_read().label();
        out.push_str(&format!("| {} | {} | `{}` |\n", t.id, label, t.command));
    }
    out
}

fn render_status(id: &str) -> Result<String, String> {
    let t = get_task(id).ok_or_else(|| format!("Task {id} not found"))?;
    t.poll_status();
    let label = t.status_read().label();

    let mut out = format!(
        "Task ID: {}\nStatus: {}\nCommand: `{}`\nLog File: {}\n\n--- LOG TAIL (Last 5KB) ---\n",
        t.id,
        label,
        t.command,
        t.log_path.display()
    );

    match fs::File::open(&t.log_path) {
        Ok(mut f) => {
            if let Ok(len) = f.metadata().map(|m| m.len()) {
                let mut buf = String::new();
                let _ = f.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL_BYTES)));
                use std::io::Read;
                let _ = f.read_to_string(&mut buf);
                out.push_str(&buf);
            }
        }
        Err(_) => out.push_str("<could not read log file>"),
    }

    Ok(out)
}

fn kill(id: &str) -> Result<String, String> {
    let t = get_task(id).ok_or_else(|| format!("Task {id} not found"))?;

    let mut status = t.status_write();
    if *status != TaskStatus::Running {
        return Ok(format!("Task {id} is already not running."));
    }
    t.child()
        .start_kill()
        .map_err(|e| format!("Failed to kill task: {e}"))?;
    *status = TaskStatus::Terminated;

    Ok(format!("Task {id} killed successfully."))
}

/// Write to a running task's stdin.
///
/// Two panic paths and one race used to live here (ADR-074):
///
/// * `Handle::current()` panics with no ambient runtime, and `block_in_place`
///   panics on a current-thread runtime — while `ferric-loop` is explicitly
///   executor-agnostic and drives mocks on `futures_executor`. Both are now
///   checked and reported as ordinary tool errors.
/// * stdin was `take()`n under one lock, written outside it, then restored
///   under a *different* acquisition, so two concurrent calls could interleave
///   and a failure in between lost the pipe permanently. The handle is now
///   borrowed in place under a single lock held for the whole write, so there is
///   nothing to lose and nothing to interleave.
fn send_input(id: &str, input: &str) -> Result<String, String> {
    let t = get_task(id).ok_or_else(|| format!("Task {id} not found"))?;

    if *t.status_read() != TaskStatus::Running {
        return Err(format!("Task {id} is not running."));
    }

    let mut child = t.child();
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| "Task does not have an open stdin pipe.".to_string())?;

    super::blocking::block_on_ambient("send_input", async {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(input.as_bytes()).await?;
        stdin.flush().await
    })?
    .map_err(|e| format!("Failed to write to task stdin: {e}"))?;

    Ok(format!("Sent input to task {id}"))
}

fn remove(id: Option<&str>) -> String {
    match id {
        Some(id) => match remove_task(id) {
            Some(t) => format!("Removed task {} (`{}`).", t.id, t.command),
            None => format!("Task {id} not found."),
        },
        None => {
            let n = remove_finished_tasks();
            format!("Removed {n} finished task(s).")
        }
    }
}
