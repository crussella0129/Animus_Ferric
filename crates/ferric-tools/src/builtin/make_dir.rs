use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, PrepareCtx, PrepareError, PrepareErrorKind, ToolPreparation,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// Create a directory (and parents) inside the workspace. Idempotent.
pub struct MakeDir;

impl Tool for MakeDir {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "make_dir".to_string(),
            description:
                "Create a directory and any missing parents. DO NOT use this to create files! Args: {\"path\": string}"
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path relative to the workspace root" }
                },
                "required": ["path"]
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
        let path = path_arg(args)
            .map_err(|message| PrepareError::new(PrepareErrorKind::InvalidArguments, message))?;
        super::controlled_file::prepare_make_dir(ctx, path)
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        std::fs::create_dir_all(&resolved).map_err(|e| format!("make_dir {path}: {e}"))?;
        Ok(format!("created directory {path}"))
    }
}
