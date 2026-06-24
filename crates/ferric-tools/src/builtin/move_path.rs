use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

/// Move/rename a file or directory within the workspace. BOTH endpoints are
/// declared as target paths so the registry boundary-checks each before the
/// handler runs (a move that escapes the workspace on either side is denied
/// with nothing renamed).
pub struct MovePath;

impl Tool for MovePath {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "move_path".to_string(),
            description:
                "Move or rename a file/directory. Args: {\"from\": string, \"to\": string}"
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source path relative to the workspace root" },
                    "to": { "type": "string", "description": "Destination path relative to the workspace root" }
                },
                "required": ["from", "to"]
            }),
            permission: PermissionLevel::Write,
            ring: 1,
        }
    }

    /// Both endpoints are guarded (the registry resolves + permission-checks
    /// each before `run`).
    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        ["from", "to"]
            .iter()
            .filter_map(|k| args.get(*k).and_then(|v| v.as_str()).map(String::from))
            .collect()
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let from = args
            .get("from")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: from".to_string())?;
        let to = args
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: to".to_string())?;
        let from_resolved = ctx
            .workspace
            .resolve(from)
            .map_err(|e| format!("boundary (from): {e}"))?;
        let to_resolved = ctx
            .workspace
            .resolve(to)
            .map_err(|e| format!("boundary (to): {e}"))?;
        if let Some(parent) = to_resolved.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir for {to}: {e}"))?;
        }
        std::fs::rename(&from_resolved, &to_resolved)
            .map_err(|e| format!("move {from} -> {to}: {e}"))?;
        Ok(format!("moved {from} -> {to}"))
    }
}
