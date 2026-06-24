use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// List a directory inside the workspace. Entries are sorted (ADR-008);
/// directories get a trailing `/`.
pub struct ListDir;

impl Tool for ListDir {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_dir".to_string(),
            description: "List directory entries (sorted). Args: {\"path\": string}".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path relative to the workspace root" }
                },
                "required": ["path"]
            }),
            permission: PermissionLevel::Read,
            ring: 0,
        }
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        let mut entries: Vec<String> = std::fs::read_dir(&resolved)
            .map_err(|e| format!("list {path}: {e}"))?
            .filter_map(|entry| entry.ok())
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                match entry.file_type() {
                    Ok(ft) if ft.is_dir() => format!("{name}/"),
                    _ => name,
                }
            })
            .collect();
        entries.sort_unstable();
        Ok(entries.join("\n"))
    }
}
