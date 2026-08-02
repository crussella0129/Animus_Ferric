use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, NoEffectKind, PrepareCtx, PrepareError, PrepareErrorKind, ToolPreparation,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::controlled_file::{compile_candidate, inspect_for_prepare};
use super::path_arg;

/// Write (create or overwrite) a UTF-8 text file inside the workspace. Legacy
/// execution creates parent directories; controlled execution requires the
/// parent chain to exist so it can be capability-pinned before publication.
pub struct WriteFile;

impl Tool for WriteFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".to_string(),
            description: "Create a new file or overwrite an existing file with UTF-8 text. Legacy execution creates missing parent directories; controlled execution requires the parent directory to exist. Args: {\"path\": string, \"content\": string}"
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

    fn control_capability(&self) -> ControlCapability {
        ControlCapability::ContentMutation
    }

    fn prepare(
        &self,
        ctx: &PrepareCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<ToolPreparation, PrepareError> {
        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required string argument: path",
                )
            })?;
        let content = args
            .get("content")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required string argument: content",
                )
            })?;
        let target = inspect_for_prepare(ctx, path, true)?;
        compile_candidate(
            target,
            content.as_bytes().to_vec(),
            NoEffectKind::Identity,
            format!("wrote {} bytes to {path}", content.len()),
        )
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
        if resolved.is_dir() {
            return Err(format!(
                "write {path} failed: path is already a directory. If you intended to write a file here, you MUST use delete_path to remove the directory first, or choose a different file name."
            ));
        }
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir for {path}: {e}"))?;
        }
        std::fs::write(&resolved, content).map_err(|e| format!("write {path}: {e}"))?;
        let mut result = format!("wrote {} bytes to {path}", content.len());
        // Best-effort syntax check: append a warning if the written code has
        // obvious syntax errors, so the model can self-correct.
        if let Some(warning) = super::check_syntax::check_syntax(&resolved) {
            result.push_str(&format!("\n⚠ {warning}"));
        }
        Ok(result)
    }
}
