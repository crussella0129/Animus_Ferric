use std::fs::File;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
                Timeout is 60 seconds. Args: {\"command\": string}".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to run" }
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
        let command = args.get("command").and_then(|v| v.as_str()).ok_or("missing 'command' argument")?;

        let temp_path = std::env::temp_dir().join(format!(
            "ferric_shell_exec_{}_{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));

        let out_file = File::create(&temp_path).map_err(|e| format!("failed to create temp file: {e}"))?;
        let err_file = out_file.try_clone().map_err(|e| format!("failed to clone temp file: {e}"))?;

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd.exe");
            c.arg("/C").arg(command);
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c
        };

        cmd.current_dir(ctx.workspace.root())
            .stdout(Stdio::from(out_file))
            .stderr(Stdio::from(err_file))
            .stdin(Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return Err(format!("failed to spawn command: {e}")),
        };

        let start = Instant::now();
        let timeout = Duration::from_secs(TIMEOUT_SECS);
        let mut timed_out = false;

        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        timed_out = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = child.kill();
                    return Err(format!("failed to wait on child: {e}"));
                }
            }
        }

        // Wait for it to fully exit after kill to avoid leaking
        let _ = child.wait();

        let mut f = File::open(&temp_path).map_err(|e| format!("failed to open temp file: {e}"))?;
        let mut full_output = String::new();
        let _ = f.read_to_string(&mut full_output);
        let _ = std::fs::remove_file(&temp_path);

        let mut result_text = if timed_out {
            format!("Command timed out after {} seconds.\nOutput:\n", TIMEOUT_SECS)
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
    }
}
