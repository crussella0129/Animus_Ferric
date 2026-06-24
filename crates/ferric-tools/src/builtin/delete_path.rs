use serde_json::json;

use ferric_core::Tier;
use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// Delete a file or directory within the workspace. A non-empty directory is
/// removed only when `recursive` is true, so a small model can't accidentally
/// nuke a tree. Declares `Write`, so the guard denylist (`.ferric`, git config,
/// ssh keys) auto-denies the same paths as `write_file`/`move_path`.
pub struct DeletePath;

impl Tool for DeletePath {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "delete_path".to_string(),
            description: "Delete a file or directory. A non-empty directory needs recursive=true. \
                Args: {\"path\": string, \"recursive\"?: boolean}"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to the workspace root" },
                    "recursive": { "type": "boolean", "description": "Remove a non-empty directory and its contents (default false)" }
                },
                "required": ["path"]
            }),
            permission: PermissionLevel::Write,
            min_tier: Tier::Nano,
        }
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        // `symlink_metadata` so a symlink is classified as a file (we remove the
        // link, never follow it to delete its target's contents).
        let meta =
            std::fs::symlink_metadata(&resolved).map_err(|e| format!("delete {path}: {e}"))?;
        if meta.is_dir() {
            let empty = std::fs::read_dir(&resolved)
                .map_err(|e| format!("delete {path}: {e}"))?
                .next()
                .is_none();
            if empty {
                std::fs::remove_dir(&resolved).map_err(|e| format!("delete {path}: {e}"))?;
                Ok(format!("deleted empty directory {path}"))
            } else if recursive {
                std::fs::remove_dir_all(&resolved).map_err(|e| format!("delete {path}: {e}"))?;
                Ok(format!("deleted directory {path} (recursive)"))
            } else {
                Err(format!(
                    "{path} is a non-empty directory; pass recursive=true to delete it"
                ))
            }
        } else {
            std::fs::remove_file(&resolved).map_err(|e| format!("delete {path}: {e}"))?;
            Ok(format!("deleted file {path}"))
        }
    }
}
