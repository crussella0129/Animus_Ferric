use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, PrepareCtx, PrepareError, PrepareErrorKind, ToolPreparation,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::controlled_read::{open_controlled_dir, validate_controlled_dir};
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

    fn control_capability(&self) -> ControlCapability {
        ControlCapability::ReadOnly
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
        let (dir, _) = open_controlled_dir(ctx.workspace, path)
            .map_err(|error| PrepareError::new(PrepareErrorKind::Io, error))?;
        let mut entries: Vec<String> = dir
            .entries()
            .map_err(|error| {
                PrepareError::new(PrepareErrorKind::Io, format!("list {path}: {error}"))
            })?
            .filter_map(Result::ok)
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                match entry.file_type() {
                    Ok(file_type) if file_type.is_dir() => format!("{name}/"),
                    _ => name,
                }
            })
            .collect();
        validate_controlled_dir(ctx.workspace, path, &dir)
            .map_err(|error| PrepareError::new(PrepareErrorKind::Io, error))?;
        entries.sort_unstable();
        Ok(ToolPreparation::immediate_read_only(entries.join("\n")))
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
