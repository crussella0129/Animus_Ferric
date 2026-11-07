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
                if let Some(s) = v.as_str()
                    && !s.starts_with('-')
                {
                    paths.push(s.to_string());
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
        let extra: Vec<&str> = args
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if let Some(refusal) = write_form(subcommand, &extra) {
            return Err(refusal);
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(ctx.workspace.root());
        cmd.arg(subcommand).args(&extra);

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

/// The forms of an admitted subcommand that write, which Read permission must
/// refuse: `--output` makes `diff` and `log` write a file, and `branch` creates,
/// deletes, renames or re-points branches unless it is listing. A positional
/// argument to `branch` is a pattern only under `--list`.
fn write_form(subcommand: &str, args: &[&str]) -> Option<String> {
    if let Some(flag) = args.iter().find(|arg| arg.starts_with("--output")) {
        return Some(format!(
            "git {subcommand} {flag} writes a file; git_read is read-only"
        ));
    }
    if subcommand != "branch" {
        return None;
    }
    let listing = args.iter().any(|arg| matches!(*arg, "-l" | "--list"));
    let listing_arg = |arg: &&str| {
        matches!(
            *arg,
            "-l" | "--list"
                | "-a"
                | "--all"
                | "-r"
                | "--remotes"
                | "-v"
                | "-vv"
                | "--verbose"
                | "--show-current"
                | "--no-color"
        ) || [
            "--sort=",
            "--format=",
            "--contains=",
            "--merged=",
            "--no-merged=",
        ]
        .iter()
        .any(|prefix| arg.starts_with(prefix))
            || (listing && !arg.starts_with('-'))
    };
    args.iter().find(|arg| !listing_arg(arg)).map(|arg| {
        format!(
            "git branch {arg} changes branches; git_read only lists them \
             (use --list for a pattern)"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::write_form;

    #[test]
    fn only_listing_forms_of_branch_and_no_output_files_are_read_only() {
        for (subcommand, args) in [
            ("branch", vec![]),
            ("branch", vec!["-a", "-v"]),
            ("branch", vec!["--list", "feature/*"]),
            ("branch", vec!["--show-current"]),
            ("branch", vec!["--contains=HEAD", "--sort=-committerdate"]),
            ("diff", vec!["--stat", "src/main.rs"]),
            ("log", vec!["-n", "5", "--oneline"]),
        ] {
            assert_eq!(write_form(subcommand, &args), None, "{subcommand} {args:?}");
        }
        for (subcommand, args) in [
            ("branch", vec!["new-branch"]),
            ("branch", vec!["-d", "main"]),
            ("branch", vec!["-D", "main"]),
            ("branch", vec!["-m", "old", "new"]),
            ("branch", vec!["--set-upstream-to=origin/main"]),
            ("branch", vec!["-f", "main", "HEAD~1"]),
            ("diff", vec!["--output=patch.txt"]),
            ("log", vec!["--output", "log.txt"]),
        ] {
            assert!(
                write_form(subcommand, &args).is_some(),
                "{subcommand} {args:?}"
            );
        }
    }
}
