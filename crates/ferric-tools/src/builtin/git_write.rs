use serde_json::json;
use std::process::Command;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

pub struct GitWrite;

impl Tool for GitWrite {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "git_write".to_string(),
            description: "Run safe git write commands: add, commit, checkout, branch. Args: {\"subcommand\": string, \"args\": [string]}".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "subcommand": { 
                        "type": "string",
                        "enum": ["add", "commit", "checkout", "branch"],
                        "description": "The git subcommand to run"
                    },
                    "args": { 
                        "type": "array", 
                        "items": { "type": "string" }, 
                        "description": "Optional flags and paths. For commit, you must provide ['-m', 'message']" 
                    }
                },
                "required": ["subcommand"]
            }),
            permission: PermissionLevel::Write,
            ring: 2,
        }
    }

    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        // Any argument that doesn't start with '-' is treated as a potential path
        // for boundary and permission checks.
        let mut paths = Vec::new();
        if let Some(arr) = args.get("args").and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    // Quick heuristic: ignore obvious flags or commit messages
                    // In a commit command, arguments after -m are messages, not paths,
                    // but guard will just check them and maybe pass or fail depending on
                    // if they hit a denylist. It's safer to yield them all if they don't start with '-'
                    // unless we do perfect parsing, which is too complex here.
                    if !s.starts_with('-') {
                        paths.push(s.to_string());
                    }
                }
            }
        }
        paths
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let subcommand = args
            .get("subcommand")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: subcommand".to_string())?;

        if !matches!(subcommand, "add" | "commit" | "checkout" | "branch") {
            return Err(format!("unsupported subcommand: {subcommand}"));
        }

        let mut cmd_args = Vec::new();
        if let Some(arr) = args.get("args").and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    cmd_args.push(s.to_string());
                }
            }
        }

        if subcommand == "commit" {
            // Prevent hanging in an editor by enforcing -m or --message or -am
            let has_message_flag = cmd_args.iter().any(|a| {
                a == "-m"
                    || a == "--message"
                    || a.starts_with("-am")
                    || (a.starts_with('-') && a.contains('m'))
            });
            if !has_message_flag {
                return Err("git commit requires a message flag (e.g. -m) to prevent hanging in the terminal editor".to_string());
            }
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(ctx.workspace.root());
        cmd.arg(subcommand);
        cmd.args(cmd_args);

        let output = cmd
            .output()
            .map_err(|e| format!("failed to execute git: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        if output.status.success() {
            if stdout.is_empty() && !stderr.is_empty() {
                Ok(stderr)
            } else {
                Ok(stdout)
            }
        } else {
            Err(format!("git {subcommand} failed:\n{stderr}\n{stdout}"))
        }
    }
}
