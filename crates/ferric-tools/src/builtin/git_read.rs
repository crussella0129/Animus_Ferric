use serde_json::json;
use std::process::Command;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

pub struct GitRead;

impl Tool for GitRead {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "git_read".to_string(),
            description: "Run safe git read commands: status, diff, log, branch. Args: {\"subcommand\": string, \"args\": [string]}".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "subcommand": { 
                        "type": "string",
                        "enum": ["status", "diff", "log", "branch"],
                        "description": "The git subcommand to run"
                    },
                    "args": { 
                        "type": "array", 
                        "items": { "type": "string" }, 
                        "description": "Optional flags and paths (e.g. ['--stat'], ['src/main.rs'])" 
                    }
                },
                "required": ["subcommand"]
            }),
            permission: PermissionLevel::Read,
            ring: 1,
        }
    }

    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        // Any argument that doesn't start with '-' is treated as a potential path
        // for boundary and permission checks.
        let mut paths = Vec::new();
        if let Some(arr) = args.get("args").and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
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

        if !matches!(subcommand, "status" | "diff" | "log" | "branch") {
            return Err(format!("unsupported subcommand: {subcommand}"));
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(ctx.workspace.root());
        cmd.arg(subcommand);

        if let Some(arr) = args.get("args").and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    cmd.arg(s);
                }
            }
        }

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
