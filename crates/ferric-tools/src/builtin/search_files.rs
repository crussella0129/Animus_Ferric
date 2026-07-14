use std::path::Path;

use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

/// Recursively search file contents for a literal substring within the
/// workspace — the navigation primitive a small model needs to find code
/// before reading or editing it. Results are `relpath:lineno:line`, sorted
/// (ADR-008) and capped (ADR-018); binary/unreadable files and noise dirs
/// (`.git`, `target`, …) are skipped.
pub struct SearchFiles;

const DEFAULT_MAX_RESULTS: usize = 50;
const NOISE_DIRS: &[&str] = &[".git", "target", "node_modules", ".ferric"];

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
                    "max_results": { "type": "integer", "description": "Cap on matching lines (default 50)" }
                },
                "required": ["query"]
            }),
            permission: PermissionLevel::Read,
            ring: 0,
        }
    }

    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        // The search root is boundary-checked by the registry before `run`.
        vec![search_path(args).to_string()]
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: query".to_string())?;
        if query.is_empty() {
            return Err("query must not be empty".to_string());
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
        search_dir(&root, ws_root, query, max_results, &mut results)?;
        Ok(results.join("\n"))
    }
}

/// The search-root argument (`path`, default `.`).
fn search_path(args: &serde_json::Value) -> &str {
    args.get("path").and_then(|v| v.as_str()).unwrap_or(".")
}

/// Recurse `dir` (sorted entries, ADR-008), appending `relpath:lineno:line`
/// matches until `max_results` is reached. Noise dirs and unreadable/binary
/// files are skipped.
fn search_dir(
    dir: &Path,
    ws_root: &Path,
    query: &str,
    max_results: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if out.len() >= max_results {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("search {}: {e}", dir.display()))?
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
            search_dir(&path, ws_root, query, max_results, out)?;
        } else {
            // Binary / unreadable files fall away here (non-UTF-8 → Err).
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let rel = path.strip_prefix(ws_root).unwrap_or(&path);
            let rel = rel.to_string_lossy().replace('\\', "/");
            for (i, line) in content.lines().enumerate() {
                if line.contains(query) {
                    out.push(format!("{}:{}:{}", rel, i + 1, line.trim_end()));
                    if out.len() >= max_results {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
