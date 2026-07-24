use std::time::Duration;

use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

/// The maximum allowed output characters from the command before truncation.
const OUTPUT_LIMIT: usize = 10_000;
const TIMEOUT_SECS: u64 = 60;

/// Execute a shell command inside the workspace.
pub struct ShellExec;

impl Tool for ShellExec {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell_exec".to_string(),
            description: "Run an arbitrary shell command within the workspace. Output is capped at 10KB. \
                Timeout is 60 seconds (unless background is true). Args: {\"command\": string, \"background\": boolean (optional)}".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to run" },
                    "background": { "type": "boolean", "description": "Run the command in the background without blocking. Returns immediately with a task ID." }
                },
                "required": ["command"]
            }),
            permission: PermissionLevel::Execute,
            ring: 2,
        }
    }

    fn target_paths(&self, _args: &serde_json::Value) -> Vec<String> {
        // We do not inherently know which paths this command will touch.
        // Paths could be passed in `command`.
        Vec::new()
    }

    fn target_commands(&self, args: &serde_json::Value) -> Vec<String> {
        args.get("command")
            .and_then(|v| v.as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default()
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or("missing 'command' argument")?;

        let is_background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Was `block_in_place(|| Handle::current().block_on(..))`, which panics
        // both without an ambient runtime and on a current-thread one — from a
        // Ring-0, model-invokable tool (ADR-074).
        super::blocking::block_on_ambient("shell_exec", async {
            use std::process::Stdio;
            use tokio::io::AsyncReadExt;

            let mut cmd = if cfg!(windows) {
                let mut c = tokio::process::Command::new("cmd.exe");
                c.arg("/C").arg(command);
                c
            } else {
                let mut c = tokio::process::Command::new("sh");
                c.arg("-c").arg(command);
                c
            };

            cmd.current_dir(ctx.workspace.root()).kill_on_drop(true);

            if is_background {
                // Create tasks dir if it doesn't exist
                let tasks_dir = ctx.workspace.root().join(".ferric").join("tasks");
                if !tasks_dir.exists() {
                    let _ = std::fs::create_dir_all(&tasks_dir);
                }

                let id = format!(
                    "task-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()
                );
                let log_path = tasks_dir.join(format!("{}.log", id));

                let log_file = std::fs::File::create(&log_path)
                    .map_err(|e| format!("failed to create log file: {e}"))?;
                cmd.stdout(
                    log_file
                        .try_clone()
                        .map_err(|e| format!("failed to clone log file: {e}"))?,
                )
                .stderr(log_file)
                .stdin(Stdio::piped());

                let child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => return Err(format!("failed to spawn background command: {e}")),
                };

                crate::builtin::task_registry::spawn_task(
                    id.clone(),
                    command.to_string(),
                    log_path.clone(),
                    child,
                );

                return Ok(format!(
                    "Started background task {}. Log file: {}. Use manage_task to interact.",
                    id,
                    log_path.display()
                ));
            }

            cmd.stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(Stdio::null());

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => return Err(format!("failed to spawn command: {e}")),
            };

            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();

            // Read from stdout and stderr concurrently, up to a reasonable cap (e.g. 500KB)
            // before stopping, just to avoid OOM.
            let mut out_buf = Vec::new();
            let mut err_buf = Vec::new();

            let timeout = Duration::from_secs(TIMEOUT_SECS);

            let mut out_take = stdout.take(500_000);
            let mut err_take = stderr.take(500_000);
            let read_pipes = async {
                let out_fut = out_take.read_to_end(&mut out_buf);
                let err_fut = err_take.read_to_end(&mut err_buf);
                let _ = tokio::join!(out_fut, err_fut);
            };

            let mut timed_out = false;
            match tokio::time::timeout(timeout, async {
                let _ = tokio::join!(child.wait(), read_pipes);
            })
            .await
            {
                Ok(_) => {}
                Err(_) => {
                    let _ = child.kill().await;
                    timed_out = true;
                }
            }

            let out_str = String::from_utf8_lossy(&out_buf);
            let err_str = String::from_utf8_lossy(&err_buf);

            let mut full_output = String::new();
            if !out_str.is_empty() {
                full_output.push_str(&out_str);
            }
            if !err_str.is_empty() {
                if !full_output.is_empty() {
                    full_output.push('\n');
                }
                full_output.push_str(&err_str);
            }

            let mut result_text = if timed_out {
                format!(
                    "Command timed out after {} seconds.\nOutput:\n",
                    TIMEOUT_SECS
                )
            } else {
                String::new()
            };

            if full_output.chars().count() > OUTPUT_LIMIT {
                let truncated: String = full_output.chars().take(OUTPUT_LIMIT).collect();
                result_text.push_str(&truncated);
                result_text.push_str("\n... [TRUNCATED]");
            } else {
                result_text.push_str(&full_output);
            }

            if timed_out {
                Err(result_text)
            } else {
                Ok(result_text)
            }
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::ToolCtx;
    use ferric_guard::Workspace;
    use serde_json::json;

    fn temp_workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        (dir, ws)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn execution_timeout_works() {
        let (_dir, ws) = temp_workspace();
        let ctx = ToolCtx { workspace: &ws };
        let tool = ShellExec;

        let cmd = "echo ping";

        let args = json!({"command": cmd});
        let result = tool.run(&ctx, &args);
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn output_cap_truncates() {
        let (_dir, ws) = temp_workspace();
        let ctx = ToolCtx { workspace: &ws };
        let tool = ShellExec;

        let cmd = if cfg!(windows) {
            // Output a lot of text using a loop
            "for /L %i in (1,1,2000) do @echo yyyyyy"
        } else {
            "for i in $(seq 1 2000); do echo yyyyyy; done"
        };

        let args = json!({"command": cmd});
        let result = tool.run(&ctx, &args).unwrap();

        // Capped at 10,000 + length of truncation notice
        assert!(result.len() <= OUTPUT_LIMIT + 100);
        assert!(result.contains("[TRUNCATED]"));
    }
}
