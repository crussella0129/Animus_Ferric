use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// Replace the first occurrence of `old_string` with `new_string` in a UTF-8
/// file inside the workspace — the targeted edit small models do far more
/// reliably than reproducing a whole file through `write_file`.
pub struct EditFile;

impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".to_string(),
            description: "Replace the first occurrence of old_string with new_string in a file. \
                Args: {\"path\": string, \"old_string\": string, \"new_string\": string}"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" },
                    "old_string": { "type": "string", "description": "Exact text to find; the first occurrence is replaced" },
                    "new_string": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
            permission: PermissionLevel::Write,
            ring: 0,
        }
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: old_string".to_string())?;
        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: new_string".to_string())?;
        if old_string.is_empty() {
            return Err("old_string must not be empty".to_string());
        }
        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        let content =
            std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))?;
        if !content.contains(old_string) {
            return Err(format!("old_string not found in {path}"));
        }
        let edited = content.replacen(old_string, new_string, 1);
        std::fs::write(&resolved, &edited).map_err(|e| format!("write {path}: {e}"))?;
        Ok(format!("edited {path} (replaced 1 occurrence)"))
    }
}
