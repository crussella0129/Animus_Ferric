use std::io::Read;
use std::path::Path;

use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, NavigationKind, PrepareCtx, PrepareError, ToolPreparation,
};
use crate::nofollow::{identity, read_options};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::navigate::{ControlledEntry, Navigator, push_matching_lines, search_path};

/// Recursively search file contents for a literal substring within the
/// workspace — the navigation primitive a small model needs to find code
/// before reading or editing it. Results are `relpath:lineno:line`, sorted
/// (ADR-008) and capped (ADR-018). Legacy execution keeps its compatibility
/// behavior of skipping binary/unreadable files; controlled preparation fails
/// on enumeration, open, or read errors so it cannot certify a false
/// exhausted/no-match observation. Noise dirs (`.git`, `target`, …) are
/// skipped in both modes.
pub struct SearchFiles;

const SEARCH: Navigator = Navigator {
    kind: NavigationKind::SearchFiles,
    tool: "search_files",
    verb: "search",
    literal: "query",
    default_max_results: 50,
    legacy_match: legacy_content_match,
    controlled_match: controlled_content_match,
};

impl Tool for SearchFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_files".to_string(),
            description: "Search file contents for a literal substring within the workspace. \
                Args: {\"query\": string, \"path\"?: string (default \".\"), \
                \"max_results\"?: number (default 50)}. Returns matching lines as \
                \"relpath:lineno:line\"."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Literal text to find" },
                    "path": { "type": "string", "description": "Directory to search, relative to the workspace root (default \".\")" },
                    "max_results": { "type": "integer", "minimum": 1, "description": "Cap on matching lines (default 50)" }
                },
                "required": ["query"]
            }),
            permission: PermissionLevel::Read,
            ring: 0,
        }
    }

    fn control_capability(&self) -> ControlCapability {
        ControlCapability::ReadOnly
    }

    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        vec![search_path(args).to_string()]
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        SEARCH.run(ctx, args)
    }

    fn prepare(
        &self,
        ctx: &PrepareCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<ToolPreparation, PrepareError> {
        SEARCH.prepare(ctx, args)
    }
}

/// Binary / unreadable files fall away here (non-UTF-8 → Err).
fn legacy_content_match(
    path: &Path,
    _name: &str,
    display: &str,
    query: &str,
    max_results: usize,
    out: &mut Vec<String>,
) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    push_matching_lines(display, &content, query, max_results, out);
}

fn controlled_content_match(
    entry: &ControlledEntry<'_>,
    query: &str,
    max_results: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    let ControlledEntry {
        dir,
        name,
        path,
        before,
    } = *entry;
    let expected = identity(before);
    let mut file = dir
        .open_with(name, &read_options())
        .map_err(|error| format!("open controlled search file {}: {error}", path.display()))?;
    let opened = file.metadata().map_err(|error| {
        format!(
            "inspect opened controlled search file {}: {error}",
            path.display()
        )
    })?;
    let after = dir.symlink_metadata(name).map_err(|error| {
        format!(
            "revalidate controlled search file {}: {error}",
            path.display()
        )
    })?;
    if !opened.is_file()
        || opened.file_type().is_symlink()
        || after.file_type().is_symlink()
        || !after.is_file()
        || identity(&opened) != expected
        || identity(&after) != expected
    {
        return Err(format!(
            "controlled search file {} changed before reading",
            path.display()
        ));
    }
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|error| format!("read controlled search file {}: {error}", path.display()))?;
    let current = dir
        .symlink_metadata(name)
        .map_err(|error| format!("revalidate controlled search file: {error}"))?;
    if current.file_type().is_symlink() || !current.is_file() || identity(&current) != expected {
        return Err("controlled search file changed during traversal".to_string());
    }
    let display = path.to_string_lossy().replace('\\', "/");
    push_matching_lines(&display, &content, query, max_results, out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::controlled_read::open_controlled_dir;

    #[test]
    fn controlled_search_fails_when_an_enumerated_entry_disappears() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("vanish.txt"), b"needle\n").unwrap();
        let workspace = ferric_guard::Workspace::new(directory.path()).unwrap();
        let (root, relative) = open_controlled_dir(&workspace, ".").unwrap();
        let mut removed = false;
        let mut results = Vec::new();

        let error = SEARCH
            .walk_controlled(
                &root,
                &relative,
                "needle",
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
        assert!(error.contains("inspect controlled search entry"));
    }

    #[test]
    fn controlled_search_reports_non_utf8_read_failure_instead_of_no_matches() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("binary.dat"), [0xff, 0xfe]).unwrap();
        let workspace = ferric_guard::Workspace::new(directory.path()).unwrap();
        let (root, relative) = open_controlled_dir(&workspace, ".").unwrap();
        let mut results = Vec::new();

        let error = SEARCH
            .walk_controlled(&root, &relative, "needle", 10, &mut results, &mut |_, _| {})
            .unwrap_err();

        assert!(results.is_empty());
        assert!(error.contains("read controlled search file"));
    }
}
