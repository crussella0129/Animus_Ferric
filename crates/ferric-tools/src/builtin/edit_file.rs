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

/// Replace the first occurrence of `old_string` with `new_string` in a UTF-8
/// file inside the workspace — the targeted edit small models do far more
/// reliably than reproducing a whole file through `write_file`.
pub struct EditFile;

impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".to_string(),
            description: "Replace only the first exact old_string span with new_string; all bytes \
                outside that matched span remain untouched. To rewrite a whole function or class, \
                include its full current definition in old_string. Args: {\"path\": string, \
                \"old_string\": string, \"new_string\": string}"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" },
                    "old_string": {
                        "type": "string",
                        "description": "Exact current text to match; only its first occurrence is replaced. Include the full current function or class definition when rewriting that whole definition"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "Replacement for the matched span; all surrounding bytes remain untouched"
                    }
                },
                "required": ["path", "old_string", "new_string"]
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
        let old_string = args
            .get("old_string")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required string argument: old_string",
                )
            })?;
        let new_string = args
            .get("new_string")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required string argument: new_string",
                )
            })?;
        if old_string.is_empty() {
            return Err(PrepareError::new(
                PrepareErrorKind::InvalidArguments,
                "old_string must not be empty",
            ));
        }

        let target = inspect_for_prepare(ctx, path, false)?;
        let content = utf8_preimage(&target, "edit_file")?;
        if !content.contains(old_string) {
            return Err(reject_unchanged(
                &target,
                NoEffectKind::MatchNotFound,
                format!("old_string not found in {path}"),
            ));
        }
        let edited = content.replacen(old_string, new_string, 1).into_bytes();
        compile_candidate(
            target,
            edited,
            NoEffectKind::Identity,
            format!("edited {path} (replaced 1 occurrence)"),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_explains_exact_span_and_whole_definition_rewrites() {
        let spec = EditFile.spec();

        assert!(
            spec.description
                .contains("Replace only the first exact old_string span")
        );
        assert!(
            spec.description
                .contains("all bytes outside that matched span remain untouched")
        );
        assert!(
            spec.description
                .contains("include its full current definition in old_string")
        );

        let properties = spec.input_schema["properties"].as_object().unwrap();
        let old_string = properties["old_string"]["description"].as_str().unwrap();
        assert!(old_string.contains("only its first occurrence is replaced"));
        assert!(old_string.contains("full current function or class definition"));

        let new_string = properties["new_string"]["description"].as_str().unwrap();
        assert!(new_string.contains("all surrounding bytes remain untouched"));
    }
}
