use std::path::Path;

use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    NavigationKind, NavigationObservation, PrepareCtx, PrepareError, PrepareErrorKind,
    ToolPreparation, normalized_relative_path, sha256_bytes,
};
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
                    "max_results": { "type": "integer", "minimum": 1, "description": "Cap on matching lines (default 50)" }
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

    fn prepare(
        &self,
        ctx: &PrepareCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<ToolPreparation, PrepareError> {
        let query = args
            .get("query")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required string argument: query",
                )
            })?;
        if query.is_empty() {
            return Err(PrepareError::new(
                PrepareErrorKind::InvalidArguments,
                "query must not be empty",
            ));
        }
        let max_results = controlled_max_results(args, DEFAULT_MAX_RESULTS)?;
        let scan_limit = max_results.checked_add(1).ok_or_else(|| {
            PrepareError::new(
                PrepareErrorKind::InvalidArguments,
                "max_results is too large to establish whether more results exist",
            )
        })?;
        let root = ctx.workspace.resolve(search_path(args)).map_err(|error| {
            PrepareError::new(PrepareErrorKind::Io, format!("boundary: {error}"))
        })?;

        let mut results = Vec::new();
        search_dir(&root, ctx.workspace.root(), query, scan_limit, &mut results)
            .map_err(|error| PrepareError::new(PrepareErrorKind::Io, error))?;
        let has_more = results.len() > max_results;
        results.truncate(max_results);
        let result_bytes = results.join("\n");
        let body = if results.is_empty() {
            "(no matches)".to_string()
        } else {
            result_bytes.clone()
        };
        let mut observation = NavigationObservation {
            kind: NavigationKind::SearchFiles,
            root: normalized_relative_path(ctx.workspace, &root)?,
            literal: query.to_string(),
            result_sha256: sha256_bytes(result_bytes.as_bytes()),
            matches: results.len() as u64,
            limit: max_results as u64,
            cap_reached: results.len() == max_results,
            has_more,
            model_truncated: false,
        };
        let header = render_navigation_header(&observation);
        require_header_fits(&header, ctx.truncation_limit)?;
        let mut full = format!("{header}{body}");
        if full.chars().count() > ctx.truncation_limit {
            observation.model_truncated = true;
            let header = render_navigation_header(&observation);
            require_header_fits(&header, ctx.truncation_limit)?;
            full = format!("{header}{body}");
        }

        Ok(ToolPreparation::navigation(full, observation))
    }
}

fn controlled_max_results(args: &serde_json::Value, default: usize) -> Result<usize, PrepareError> {
    let Some(value) = args.get("max_results") else {
        return Ok(default);
    };
    let value = value.as_u64().filter(|value| *value > 0).ok_or_else(|| {
        PrepareError::new(
            PrepareErrorKind::InvalidArguments,
            "max_results must be a positive integer",
        )
    })?;
    usize::try_from(value).map_err(|_| {
        PrepareError::new(
            PrepareErrorKind::InvalidArguments,
            "max_results is too large for this platform",
        )
    })
}

fn render_navigation_header(observation: &NavigationObservation) -> String {
    format!(
        "[ferric:navigation_observation:v1]\nkind=search_files\nroot={}\nliteral={}\nresult_sha256={}\nmatches={}\nlimit={}\ncap_reached={}\nhas_more={}\nmodel_truncated={}\n[/ferric:navigation_observation:v1]\n",
        serde_json::to_string(&observation.root).expect("serializing a string cannot fail"),
        serde_json::to_string(&observation.literal).expect("serializing a string cannot fail"),
        observation.result_sha256,
        observation.matches,
        observation.limit,
        u8::from(observation.cap_reached),
        u8::from(observation.has_more),
        u8::from(observation.model_truncated),
    )
}

fn require_header_fits(header: &str, limit: usize) -> Result<(), PrepareError> {
    if header.chars().count() > limit {
        return Err(PrepareError::new(
            PrepareErrorKind::OutputLimitTooSmall,
            format!(
                "model output limit {limit} is too small for the search_files evidence envelope"
            ),
        ));
    }
    Ok(())
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
