use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::args;
use super::git_read::{git_args, git_path_args, run_git};

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
        git_path_args(args)
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let subcommand = args::string(args, "subcommand")?;
        if !matches!(subcommand, "add" | "commit" | "checkout" | "branch") {
            return Err(format!("unsupported subcommand: {subcommand}"));
        }
        let cmd_args = git_args(args);
        if subcommand == "commit" {
            // Prevent hanging in an editor by enforcing -m or --message or -am
            let has_message_flag = cmd_args.iter().any(|a| {
                *a == "-m"
                    || *a == "--message"
                    || a.starts_with("-am")
                    || (a.starts_with('-') && a.contains('m'))
            });
            if !has_message_flag {
                return Err("git commit requires a message flag (e.g. -m) to prevent hanging in the terminal editor".to_string());
            }
        }
        run_git(ctx, subcommand, &cmd_args)
    }
}
