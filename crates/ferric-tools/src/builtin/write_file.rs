use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// Write (create or overwrite) a UTF-8 text file inside the workspace,
/// creating parent directories as needed.
pub struct WriteFile;

impl Tool for WriteFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".to_string(),
            description: "Write a UTF-8 text file (creates parents). Args: {\"path\": string, \"content\": string}"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" },
                    "content": { "type": "string", "description": "Full file content" }
                },
                "required": ["path", "content"]
            }),
            permission: PermissionLevel::Write,
            ring: 0,
        }
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: content".to_string())?;
        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir for {path}: {e}"))?;
        }
        std::fs::write(&resolved, content).map_err(|e| format!("write {path}: {e}"))?;
        Ok(format!("wrote {} bytes to {path}", content.len()))
    }
}
