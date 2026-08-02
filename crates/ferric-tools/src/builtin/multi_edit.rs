use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, NoEffectKind, PrepareCtx, PrepareError, PrepareErrorKind, ToolPreparation,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::controlled_file::{
    compile_candidate, inspect_for_prepare, reject_unchanged, utf8_preimage,
};
use super::path_arg;

/// Apply an ordered batch of first-occurrence replacements to one file,
/// as one candidate: every edit is validated and applied to an in-memory
/// working copy first. Controlled execution atomically publishes that candidate
/// at the target path; a validation failure leaves the file byte-identical. Ring 2 ("plan & apply structured
/// changes"): a capable model expresses a coherent multi-part change in one call,
/// instead of a sequence of single `edit_file`s. Edits apply in order, so a later
/// edit may touch text an earlier one inserted.
pub struct MultiEdit;

impl Tool for MultiEdit {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "multi_edit".to_string(),
            description: "Apply an ordered list of first-occurrence replacements to one file. \
                Controlled execution atomically publishes one fully validated candidate at the target path. Args: {\"path\": string, \"edits\": [{\"old_string\": \
                string, \"new_string\": string}, ...]}. Edits apply in order to a working copy; \
                if any old_string is missing the file is left unchanged."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" },
                    "edits": {
                        "type": "array",
                        "description": "Ordered edits; each replaces the first occurrence of old_string",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": { "type": "string", "description": "Exact text to find" },
                                "new_string": { "type": "string", "description": "Replacement text" }
                            },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["path", "edits"]
            }),
            permission: PermissionLevel::Write,
            ring: 2,
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
        let edits = args
            .get("edits")
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required array argument: edits",
                )
            })?;
        if edits.is_empty() {
            return Err(PrepareError::new(
                PrepareErrorKind::InvalidArguments,
                "edits must not be empty",
            ));
        }

        let target = inspect_for_prepare(ctx, path, false)?;
        let mut working = utf8_preimage(&target, "multi_edit")?.to_string();
        for (index, edit) in edits.iter().enumerate() {
            let old_string = edit
                .get("old_string")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    PrepareError::new(
                        PrepareErrorKind::InvalidArguments,
                        format!("edit {index}: missing string field old_string"),
                    )
                })?;
            let new_string = edit
                .get("new_string")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    PrepareError::new(
                        PrepareErrorKind::InvalidArguments,
                        format!("edit {index}: missing string field new_string"),
                    )
                })?;
            if old_string.is_empty() {
                return Err(PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    format!("edit {index}: old_string must not be empty"),
                ));
            }
            if !working.contains(old_string) {
                return Err(reject_unchanged(
                    &target,
                    NoEffectKind::MatchNotFound,
                    format!("edit {index}: old_string not found in {path}"),
                ));
            }
            working = working.replacen(old_string, new_string, 1);
        }

        compile_candidate(
            target,
            working.into_bytes(),
            NoEffectKind::NetZeroBatch,
            format!("applied {} edits to {path}", edits.len()),
        )
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let edits = args
            .get("edits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "missing required array argument: edits".to_string())?;
        if edits.is_empty() {
            return Err("edits must not be empty".to_string());
        }

        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        let mut working =
            std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))?;

        // Validate + apply every edit to the working copy BEFORE any write, so a
        // failure leaves the file untouched.
        for (i, edit) in edits.iter().enumerate() {
            let old_string = edit
                .get("old_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("edit {i}: missing string field old_string"))?;
            let new_string = edit
                .get("new_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("edit {i}: missing string field new_string"))?;
            if old_string.is_empty() {
                return Err(format!("edit {i}: old_string must not be empty"));
            }
            if !working.contains(old_string) {
                return Err(format!("edit {i}: old_string not found in {path}"));
            }
            working = working.replacen(old_string, new_string, 1);
        }

        std::fs::write(&resolved, &working).map_err(|e| format!("write {path}: {e}"))?;
        Ok(format!("applied {} edits to {path}", edits.len()))
    }
}
