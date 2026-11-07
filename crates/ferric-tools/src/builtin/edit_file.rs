use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{ControlCapability, NoEffectKind, PrepareCtx, PrepareError, ToolPreparation};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::args::{self, ArgError};
use super::controlled_file::{EditFailure, compile_candidate, inspect_for_prepare, utf8_preimage};

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
        let (path, old_string, new_string) = edit_args(args)?;
        let target = inspect_for_prepare(ctx, path, false)?;
        let content = utf8_preimage(&target, "edit_file")?;
        let edited = replace_first(content, old_string, new_string, path)
            .map_err(|failure| failure.rejected(&target))?;
        compile_candidate(
            target,
            edited.into_bytes(),
            NoEffectKind::Identity,
            format!("edited {path} (replaced 1 occurrence)"),
        )
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let (path, old_string, new_string) = edit_args(args)?;
        let resolved = ctx.resolve(path)?;
        let content =
            std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))?;
        let edited = replace_first(&content, old_string, new_string, path)?;
        std::fs::write(&resolved, &edited).map_err(|e| format!("write {path}: {e}"))?;
        Ok(format!("edited {path} (replaced 1 occurrence)"))
    }
}

/// `(path, old_string, new_string)`, with a non-empty `old_string`.
fn edit_args(args: &serde_json::Value) -> Result<(&str, &str, &str), ArgError> {
    let path = args::string(args, "path")?;
    let old_string = args::string(args, "old_string")?;
    let new_string = args::string(args, "new_string")?;
    if old_string.is_empty() {
        return Err(args::invalid("old_string must not be empty"));
    }
    Ok((path, old_string, new_string))
}

/// Replace the first occurrence of `old_string` in `content`.
fn replace_first(
    content: &str,
    old_string: &str,
    new_string: &str,
    path: &str,
) -> Result<String, EditFailure> {
    if !content.contains(old_string) {
        return Err(EditFailure::NotFound(format!(
            "old_string not found in {path}"
        )));
    }
    Ok(content.replacen(old_string, new_string, 1))
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
