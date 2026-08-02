use std::path::Path;

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::Dir;
use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, NavigationKind, NavigationObservation, PrepareCtx, PrepareError,
    PrepareErrorKind, ToolPreparation, normalized_relative_path, sha256_bytes,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::controlled_read::{open_controlled_dir, validate_controlled_dir};

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

    fn prepare(
        &self,
        ctx: &PrepareCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<ToolPreparation, PrepareError> {
        let pattern = args
            .get("pattern")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required string argument: pattern",
                )
            })?;
        if pattern.is_empty() {
            return Err(PrepareError::new(
                PrepareErrorKind::InvalidArguments,
                "pattern must not be empty",
            ));
        }
        let max_results = controlled_max_results(args, DEFAULT_MAX_RESULTS)?;
        let scan_limit = max_results.checked_add(1).ok_or_else(|| {
            PrepareError::new(
                PrepareErrorKind::InvalidArguments,
                "max_results is too large to establish whether more results exist",
            )
        })?;
        let (root_dir, relative_root) = open_controlled_dir(ctx.workspace, search_path(args))
            .map_err(|error| PrepareError::new(PrepareErrorKind::Io, error))?;
        let mut results = Vec::new();
        find_dir_controlled(&root_dir, &relative_root, pattern, scan_limit, &mut results)
            .map_err(|error| PrepareError::new(PrepareErrorKind::Io, error))?;
        validate_controlled_dir(ctx.workspace, search_path(args), &root_dir)
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
            kind: NavigationKind::FindFiles,
            root: normalized_relative_path(
                ctx.workspace,
                &ctx.workspace.root().join(&relative_root),
            )?,
            literal: pattern.to_string(),
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
        "[ferric:navigation_observation:v1]\nkind=find_files\nroot={}\nliteral={}\nresult_sha256={}\nmatches={}\nlimit={}\ncap_reached={}\nhas_more={}\nmodel_truncated={}\n[/ferric:navigation_observation:v1]\n",
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
            format!("model output limit {limit} is too small for the find_files evidence envelope"),
        ));
    }
    Ok(())
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

fn find_dir_controlled(
    dir: &Dir,
    relative: &Path,
    pattern: &str,
    max_results: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if out.len() >= max_results {
        return Ok(());
    }
    let mut entries: Vec<_> = dir
        .entries()
        .map_err(|error| format!("find {}: {error}", relative.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);

    for entry in entries {
        if out.len() >= max_results {
            break;
        }
        let name = entry.file_name();
        let before = match dir.symlink_metadata(&name) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if before.file_type().is_symlink() {
            continue;
        }
        let path = relative.join(&name);
        if before.is_dir() {
            let lossy = name.to_string_lossy();
            if NOISE_DIRS.contains(&lossy.as_ref()) {
                continue;
            }
            let identity = (MetadataExt::dev(&before), MetadataExt::ino(&before));
            let Ok(child) = dir.open_dir_nofollow(&name) else {
                continue;
            };
            let Ok(opened) = child.dir_metadata() else {
                continue;
            };
            let Ok(after) = dir.symlink_metadata(&name) else {
                continue;
            };
            if !opened.is_dir()
                || after.file_type().is_symlink()
                || !after.is_dir()
                || (MetadataExt::dev(&opened), MetadataExt::ino(&opened)) != identity
                || (MetadataExt::dev(&after), MetadataExt::ino(&after)) != identity
            {
                continue;
            }
            find_dir_controlled(&child, &path, pattern, max_results, out)?;
            let current = dir
                .symlink_metadata(&name)
                .map_err(|error| format!("revalidate controlled find directory: {error}"))?;
            if current.file_type().is_symlink()
                || !current.is_dir()
                || (MetadataExt::dev(&current), MetadataExt::ino(&current)) != identity
            {
                return Err("controlled find directory changed during traversal".to_string());
            }
        } else if before.is_file() && name.to_string_lossy().contains(pattern) {
            let identity = (MetadataExt::dev(&before), MetadataExt::ino(&before));
            let Ok(after) = dir.symlink_metadata(&name) else {
                continue;
            };
            if after.file_type().is_symlink()
                || !after.is_file()
                || (MetadataExt::dev(&after), MetadataExt::ino(&after)) != identity
            {
                continue;
            }
            out.push(path.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
