use std::path::Path;

use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, NavigationKind, PrepareCtx, PrepareError, ToolPreparation,
};
use crate::nofollow::identity;
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::navigate::{ControlledEntry, Navigator, search_path};

/// Recursively find files whose **name** contains a literal substring within the
/// workspace — the name-search companion to `search_files`' content search. A
/// small model uses this to locate a file ("the config one") before reading or
/// editing it. Results are workspace-relative paths, sorted (ADR-008) and capped
/// (ADR-018); noise dirs (`.git`, `target`, …) are skipped.
pub struct FindFiles;

const FIND: Navigator = Navigator {
    kind: NavigationKind::FindFiles,
    tool: "find_files",
    verb: "find",
    literal: "pattern",
    default_max_results: 100,
    legacy_match: legacy_name_match,
    controlled_match: controlled_name_match,
};

impl Tool for FindFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "find_files".to_string(),
            description: "Find files whose NAME contains a literal substring within the \
                workspace (the name-search companion to search_files' content search). \
                Args: {\"pattern\": string, \"path\"?: string (default \".\"), \
                \"max_results\"?: number (default 100)}. Returns matching file paths, \
                one per line, relative to the workspace root."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Literal text to match against file names" },
                    "path": { "type": "string", "description": "Directory to search, relative to the workspace root (default \".\")" },
                    "max_results": { "type": "integer", "minimum": 1, "description": "Cap on matching files (default 100)" }
                },
                "required": ["pattern"]
            }),
            permission: PermissionLevel::Read,
            ring: 1,
        }
    }

    fn control_capability(&self) -> ControlCapability {
        ControlCapability::ReadOnly
    }

    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        vec![search_path(args).to_string()]
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        FIND.run(ctx, args)
    }

    fn prepare(
        &self,
        ctx: &PrepareCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<ToolPreparation, PrepareError> {
        FIND.prepare(ctx, args)
    }
}

fn legacy_name_match(
    _path: &Path,
    name: &str,
    display: &str,
    pattern: &str,
    _max_results: usize,
    out: &mut Vec<String>,
) {
    if name.contains(pattern) {
        out.push(display.to_string());
    }
}

fn controlled_name_match(
    entry: &ControlledEntry<'_>,
    pattern: &str,
    _max_results: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if !entry.name.to_string_lossy().contains(pattern) {
        return Ok(());
    }
    let expected = identity(entry.before);
    let after = entry.dir.symlink_metadata(entry.name).map_err(|error| {
        format!(
            "revalidate controlled find file {}: {error}",
            entry.path.display()
        )
    })?;
    if after.file_type().is_symlink() || !after.is_file() || identity(&after) != expected {
        return Err(format!(
            "controlled find file {} changed during traversal",
            entry.path.display()
        ));
    }
    out.push(entry.path.to_string_lossy().replace('\\', "/"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::controlled_read::open_controlled_dir;

    #[test]
    fn controlled_find_fails_when_an_enumerated_entry_disappears() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("vanish.txt"), b"data\n").unwrap();
        let workspace = ferric_guard::Workspace::new(directory.path()).unwrap();
        let (root, relative) = open_controlled_dir(&workspace, ".").unwrap();
        let mut removed = false;
        let mut results = Vec::new();

        let error = FIND
            .walk_controlled(
                &root,
                &relative,
                "vanish",
                10,
                &mut results,
                &mut |dir, name| {
                    if !removed && name.to_string_lossy() == "vanish.txt" {
                        removed = true;
                        dir.remove_file(name).unwrap();
                    }
                },
            )
            .unwrap_err();

        assert!(removed);
        assert!(results.is_empty());
        assert!(error.contains("inspect controlled find entry"));
    }
}
