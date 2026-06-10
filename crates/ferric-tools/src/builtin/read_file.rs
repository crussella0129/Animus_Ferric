use serde_json::json;

use ferric_core::Tier;
use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// Read a UTF-8 text file inside the workspace.
pub struct ReadFile;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_string(),
            description: "Read a UTF-8 text file. Args: {\"path\": string}".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" }
                },
                "required": ["path"]
            }),
            permission: PermissionLevel::Read,
            min_tier: Tier::Nano,
        }
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))
    }
}
