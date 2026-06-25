use std::path::Path;

use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

/// Recursively find files whose **name** contains a literal substring within the
/// workspace — the name-search companion to `search_files`' content search. A
/// small model uses this to locate a file ("the config one") before reading or
/// editing it. Results are workspace-relative paths, sorted (ADR-008) and capped
/// (ADR-018); noise dirs (`.git`, `target`, …) are skipped.
pub struct FindFiles;

const DEFAULT_MAX_RESULTS: usize = 100;
const NOISE_DIRS: &[&str] = &[".git", "target", "node_modules", ".ferric"];

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
                    "max_results": { "type": "integer", "description": "Cap on matching files (default 100)" }
                },
                "required": ["pattern"]
            }),
            permission: PermissionLevel::Read,
            ring: 1,
        }
    }

    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        // The search root is boundary-checked by the registry before `run`.
        vec![search_path(args).to_string()]
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: pattern".to_string())?;
        if pattern.is_empty() {
            return Err("pattern must not be empty".to_string());
        }
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS);

        let root = ctx
            .workspace
            .resolve(search_path(args))
            .map_err(|e| format!("boundary: {e}"))?;
        let ws_root = ctx.workspace.root();

        let mut results: Vec<String> = Vec::new();
        find_dir(&root, ws_root, pattern, max_results, &mut results)?;
        Ok(results.join("\n"))
    }
}

/// The search-root argument (`path`, default `.`).
fn search_path(args: &serde_json::Value) -> &str {
    args.get("path").and_then(|v| v.as_str()).unwrap_or(".")
}

/// Recurse `dir` (sorted entries, ADR-008), appending the workspace-relative
/// path of every file whose name contains `pattern`, until `max_results`. Noise
/// dirs are skipped.
fn find_dir(
    dir: &Path,
    ws_root: &Path,
    pattern: &str,
    max_results: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if out.len() >= max_results {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("find {}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    // Deterministic walk order (ADR-008).
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if out.len() >= max_results {
            break;
        }
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if is_dir {
            let name_ref: &str = name.as_ref();
            if NOISE_DIRS.contains(&name_ref) {
                continue;
            }
            find_dir(&path, ws_root, pattern, max_results, out)?;
        } else if name.contains(pattern) {
            let rel = path.strip_prefix(ws_root).unwrap_or(&path);
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
