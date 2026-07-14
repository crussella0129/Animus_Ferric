use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// Read a UTF-8 text file inside the workspace, optionally paginated by lines.
pub struct ReadFile;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_string(),
            description: "Read a UTF-8 text file. Use start_line and end_line to paginate large files \
                and protect your context budget. Args: {\"path\": string, \"start_line\"?: number, \
                \"end_line\"?: number} (Lines are 1-indexed, inclusive).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" },
                    "start_line": { "type": "integer", "description": "Optional starting line number (1-indexed, inclusive)" },
                    "end_line": { "type": "integer", "description": "Optional ending line number (1-indexed, inclusive)" }
                },
                "required": ["path"]
            }),
            permission: PermissionLevel::Read,
            ring: 0,
        }
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let start_line = args.get("start_line").and_then(|v| v.as_u64()).map(|n| n as usize);
        let end_line = args.get("end_line").and_then(|v| v.as_u64()).map(|n| n as usize);
        
        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
            
        let content = std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))?;
        
        if start_line.is_none() && end_line.is_none() {
            return Ok(content);
        }
        
        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(usize::MAX);
        
        let lines: Vec<&str> = content.lines().skip(start).take(end.saturating_sub(start)).collect();
        Ok(lines.join("\n"))
    }
}
