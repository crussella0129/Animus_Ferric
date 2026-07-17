use std::fs;
use std::io::{Seek, SeekFrom};
use serde_json::json;

use ferric_guard::PermissionLevel;
use crate::spec::{Tool, ToolCtx, ToolSpec};
use super::task_registry::{get_task, list_tasks, TaskStatus};

pub struct ManageTask;

impl Tool for ManageTask {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "manage_task".to_string(),
            description: "Manage background tasks. Actions: 'list' (all tasks), 'status' (view logs and status of a specific task), 'kill' (terminate a task), 'send_input' (send stdin to a task).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "status", "kill", "send_input"] },
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
        let action = args.get("action").and_then(|v| v.as_str()).ok_or("missing 'action' argument")?;
        
        match action {
            "list" => {
                let tasks = list_tasks();
                if tasks.is_empty() {
                    return Ok("No background tasks running or tracked.".to_string());
                }
                
                let mut out = String::from("| ID | Status | Command |\n|---|---|---|\n");
                for t in tasks {
                    let mut status = t.status.write().unwrap();
                    
                    // Poll if it's still running
                    if *status == TaskStatus::Running {
                        let mut child = t.child.lock().unwrap();
                        if let Ok(Some(exit_status)) = child.try_wait() {
                            let code = exit_status.code().unwrap_or(-1);
                            *status = TaskStatus::Completed { exit_code: code };
                        }
                    }
                    
                    let status_str = match &*status {
                        TaskStatus::Running => "Running",
                        TaskStatus::Completed { exit_code } => &format!("Completed (Exit {})", exit_code),
                        TaskStatus::Terminated => "Terminated",
                        TaskStatus::Error(e) => &format!("Error: {}", e),
                    };
                    
                    out.push_str(&format!("| {} | {} | `{}` |\n", t.id, status_str, t.command));
                }
                Ok(out)
            }
            "status" => {
                let id = args.get("task_id").and_then(|v| v.as_str()).ok_or("missing 'task_id'")?;
                let t = get_task(id).ok_or_else(|| format!("Task {} not found", id))?;
                
                let mut status = t.status.write().unwrap();
                if *status == TaskStatus::Running {
                    let mut child = t.child.lock().unwrap();
                    if let Ok(Some(exit_status)) = child.try_wait() {
                        let code = exit_status.code().unwrap_or(-1);
                        *status = TaskStatus::Completed { exit_code: code };
                    }
                }
                
                let status_str = match &*status {
                    TaskStatus::Running => "Running",
                    TaskStatus::Completed { exit_code } => &format!("Completed (Exit {})", exit_code),
                    TaskStatus::Terminated => "Terminated",
                    TaskStatus::Error(e) => &format!("Error: {}", e),
                };
                
                let mut out = format!("Task ID: {}\nStatus: {}\nCommand: `{}`\nLog File: {}\n\n--- LOG TAIL (Last 5KB) ---\n", t.id, status_str, t.command, t.log_path.display());
                
                if let Ok(mut f) = fs::File::open(&t.log_path) {
                    if let Ok(len) = f.metadata().map(|m| m.len()) {
                        let mut buf = String::new();
                        let start = len.saturating_sub(5000);
                        let _ = f.seek(SeekFrom::Start(start));
                        use std::io::Read;
                        let _ = f.read_to_string(&mut buf);
                        out.push_str(&buf);
                    }
                } else {
                    out.push_str("<could not read log file>");
                }
                
                Ok(out)
            }
            "kill" => {
                let id = args.get("task_id").and_then(|v| v.as_str()).ok_or("missing 'task_id'")?;
                let t = get_task(id).ok_or_else(|| format!("Task {} not found", id))?;
                
                let mut status = t.status.write().unwrap();
                if *status != TaskStatus::Running {
                    return Ok(format!("Task {} is already not running.", id));
                }
                
                let mut child = t.child.lock().unwrap();
                if let Err(e) = child.start_kill() {
                    return Err(format!("Failed to kill task: {}", e));
                }
                *status = TaskStatus::Terminated;
                
                Ok(format!("Task {} killed successfully.", id))
            }
            "send_input" => {
                let id = args.get("task_id").and_then(|v| v.as_str()).ok_or("missing 'task_id'")?;
                let input = args.get("input").and_then(|v| v.as_str()).ok_or("missing 'input'")?;
                let t = get_task(id).ok_or_else(|| format!("Task {} not found", id))?;
                
                let status = t.status.read().unwrap();
                if *status != TaskStatus::Running {
                    return Err(format!("Task {} is not running.", id));
                }
                
                let stdin_opt = {
                    let mut child = t.child.lock().unwrap();
                    child.stdin.take()
                };
                
                if let Some(mut stdin) = stdin_opt {
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            use tokio::io::AsyncWriteExt;
                            let _ = stdin.write_all(input.as_bytes()).await;
                            let _ = stdin.flush().await;
                        })
                    });
                    
                    // Put it back
                    let mut child = t.child.lock().unwrap();
                    child.stdin = Some(stdin);
                    
                    Ok(format!("Sent input to task {}", id))
                } else {
                    Err("Task does not have an open stdin pipe.".to_string())
                }
            }
            _ => Err(format!("Unknown action: {}", action)),
        }
    }
}
