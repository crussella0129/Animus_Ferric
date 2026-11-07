use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{ControlCapability, NoEffectKind, PrepareCtx, PrepareError, ToolPreparation};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::args::{self, ArgError};
use super::controlled_file::{EditFailure, compile_candidate, inspect_for_prepare, utf8_preimage};

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
        let (path, edits) = multi_edit_args(args)?;
        let target = inspect_for_prepare(ctx, path, false)?;
        let content = utf8_preimage(&target, "multi_edit")?;
        let edited =
            apply_edits(content, edits, path).map_err(|failure| failure.rejected(&target))?;
        compile_candidate(
            target,
            edited.into_bytes(),
            NoEffectKind::NetZeroBatch,
            format!("applied {} edits to {path}", edits.len()),
        )
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let (path, edits) = multi_edit_args(args)?;
        let resolved = ctx.resolve(path)?;
        let content =
            std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))?;
        let edited = apply_edits(&content, edits, path)?;
        std::fs::write(&resolved, &edited).map_err(|e| format!("write {path}: {e}"))?;
        Ok(format!("applied {} edits to {path}", edits.len()))
    }
}

/// `(path, edits)`, with at least one edit.
fn multi_edit_args(args: &serde_json::Value) -> Result<(&str, &Vec<serde_json::Value>), ArgError> {
    let path = args::string(args, "path")?;
    let edits = args::array(args, "edits")?;
    if edits.is_empty() {
        return Err(args::invalid("edits must not be empty"));
    }
    Ok((path, edits))
}

/// Validate and apply every edit, in order, to a working copy of `content`,
/// so a failure leaves the file untouched.
fn apply_edits(
    content: &str,
    edits: &[serde_json::Value],
    path: &str,
) -> Result<String, EditFailure> {
    let mut working = content.to_string();
    for (index, edit) in edits.iter().enumerate() {
        let field = |key: &str| {
            edit.get(key)
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    EditFailure::Invalid(format!("edit {index}: missing string field {key}"))
                })
        };
        let old_string = field("old_string")?;
        let new_string = field("new_string")?;
        if old_string.is_empty() {
            return Err(EditFailure::Invalid(format!(
                "edit {index}: old_string must not be empty"
            )));
        }
        if !working.contains(old_string) {
            return Err(EditFailure::NotFound(format!(
                "edit {index}: old_string not found in {path}"
            )));
        }
        working = working.replacen(old_string, new_string, 1);
    }
    Ok(working)
}
