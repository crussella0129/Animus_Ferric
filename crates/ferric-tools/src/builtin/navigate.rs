//! The navigation core shared by `find_files` (file names) and `search_files`
//! (file contents): argument parsing, the legacy and capability-relative
//! walkers, and the evidence envelope. Each tool supplies only how one
//! regular file is matched.

use std::ffi::OsString;
use std::path::Path;

use cap_fs_ext::DirExt;
use cap_std::fs::{Dir, Metadata};

use crate::control::{
    NavigationKind, NavigationObservation, PrepareCtx, PrepareError, PrepareErrorKind,
    ToolPreparation, evidence_envelope, normalized_relative_path, sha256_bytes,
};
use crate::nofollow::identity;
use crate::spec::ToolCtx;

use super::args;
use super::controlled_read::{open_controlled_dir, validate_controlled_dir};

/// Directories neither walker descends into.
const NOISE_DIRS: &[&str] = &[".git", "target", "node_modules", ".ferric"];

/// One regular file reached by the controlled walk, already classified from
/// `before` (its no-follow metadata) as a plain file.
pub(super) struct ControlledEntry<'a> {
    pub dir: &'a Dir,
    pub name: &'a OsString,
    pub path: &'a Path,
    pub before: &'a Metadata,
}

/// A navigation tool: its identity plus how it matches one file.
pub(super) struct Navigator {
    pub kind: NavigationKind,
    /// The tool name rendered in the evidence header.
    pub tool: &'static str,
    /// The verb used in walker diagnostics (`find`, `search`).
    pub verb: &'static str,
    /// The argument carrying the literal to match.
    pub literal: &'static str,
    pub default_max_results: usize,
    /// Legacy walk: match a non-directory entry given its path, file name and
    /// workspace-relative display path.
    pub legacy_match: fn(&Path, &str, &str, &str, usize, &mut Vec<String>),
    /// Controlled walk: match a plain regular file.
    pub controlled_match:
        fn(&ControlledEntry<'_>, &str, usize, &mut Vec<String>) -> Result<(), String>,
}

/// The search-root argument (`path`, default `.`). The registry
/// boundary-checks it before `run`.
pub(super) fn search_path(args: &serde_json::Value) -> &str {
    args.get("path").and_then(|v| v.as_str()).unwrap_or(".")
}

/// Append `display:lineno:line` for each line of `content` containing
/// `query`, until `max_results`.
pub(super) fn push_matching_lines(
    display: &str,
    content: &str,
    query: &str,
    max_results: usize,
    out: &mut Vec<String>,
) {
    for (index, line) in content.lines().enumerate() {
        if line.contains(query) {
            out.push(format!("{}:{}:{}", display, index + 1, line.trim_end()));
            if out.len() >= max_results {
                break;
            }
        }
    }
}

impl Navigator {
    pub(super) fn run(
        &self,
        ctx: &ToolCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<String, String> {
        let literal = args::string(args, self.literal)?;
        if literal.is_empty() {
            return Err(format!("{} must not be empty", self.literal));
        }
        let max_results = args::legacy_count(args, "max_results", self.default_max_results);
        let root = ctx.resolve(search_path(args))?;
        let mut results = Vec::new();
        self.walk_legacy(
            &root,
            ctx.workspace.root(),
            literal,
            max_results,
            &mut results,
        )?;
        Ok(results.join("\n"))
    }

    pub(super) fn prepare(
        &self,
        ctx: &PrepareCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<ToolPreparation, PrepareError> {
        let literal = args::string(args, self.literal)?;
        if literal.is_empty() {
            return Err(args::invalid(format!("{} must not be empty", self.literal)).into());
        }
        let max_results = match args::optional_positive(args, "max_results")? {
            None => self.default_max_results,
            Some(value) => usize::try_from(value)
                .map_err(|_| args::invalid("max_results is too large for this platform"))?,
        };
        let scan_limit = max_results.checked_add(1).ok_or_else(|| {
            args::invalid("max_results is too large to establish whether more results exist")
        })?;
        let io = |error| PrepareError::new(PrepareErrorKind::Io, error);
        let (root_dir, relative_root) =
            open_controlled_dir(ctx.workspace, search_path(args)).map_err(io)?;
        let mut results = Vec::new();
        self.walk_controlled(
            &root_dir,
            &relative_root,
            literal,
            scan_limit,
            &mut results,
            &mut |_, _| {},
        )
        .map_err(io)?;
        validate_controlled_dir(ctx.workspace, search_path(args), &root_dir).map_err(io)?;
        let has_more = results.len() > max_results;
        results.truncate(max_results);
        let result_bytes = results.join("\n");
        let body = if results.is_empty() {
            "(no matches)"
        } else {
            result_bytes.as_str()
        };
        let mut observation = NavigationObservation {
            kind: self.kind,
            root: normalized_relative_path(
                ctx.workspace,
                &ctx.workspace.root().join(&relative_root),
            )?,
            literal: literal.to_string(),
            result_sha256: sha256_bytes(result_bytes.as_bytes()),
            matches: results.len() as u64,
            limit: max_results as u64,
            cap_reached: results.len() == max_results,
            has_more,
            model_truncated: false,
        };
        let full = evidence_envelope(
            self.tool,
            ctx.truncation_limit,
            &mut observation,
            body,
            |observation| self.render_header(observation),
            |observation| observation.model_truncated = true,
        )?;
        Ok(ToolPreparation::navigation(full, observation))
    }

    fn render_header(&self, observation: &NavigationObservation) -> String {
        format!(
            "[ferric:navigation_observation:v1]\nkind={}\nroot={}\nliteral={}\nresult_sha256={}\nmatches={}\nlimit={}\ncap_reached={}\nhas_more={}\nmodel_truncated={}\n[/ferric:navigation_observation:v1]\n",
            self.tool,
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

    /// Recurse `dir` (sorted entries, ADR-008), matching every non-directory
    /// entry until `max_results`. Noise dirs are skipped.
    fn walk_legacy(
        &self,
        dir: &Path,
        ws_root: &Path,
        literal: &str,
        max_results: usize,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if out.len() >= max_results {
            return Ok(());
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{} {}: {e}", self.verb, dir.display()))?
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
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if NOISE_DIRS.contains(&name.as_ref()) {
                    continue;
                }
                self.walk_legacy(&path, ws_root, literal, max_results, out)?;
            } else {
                let rel = path.strip_prefix(ws_root).unwrap_or(&path);
                let display = rel.to_string_lossy().replace('\\', "/");
                (self.legacy_match)(&path, &name, &display, literal, max_results, out);
            }
        }
        Ok(())
    }

    /// Capability-relative controlled traversal. Symlinks, reparse points,
    /// and non-regular entries are skipped; a raced replacement cannot escape
    /// the retained directory handle or be followed by a final open. Any
    /// enumeration, open, or read error fails the walk so it cannot certify a
    /// false exhausted/no-match observation. `before_metadata` is a test seam.
    pub(super) fn walk_controlled<F>(
        &self,
        dir: &Dir,
        relative: &Path,
        literal: &str,
        max_results: usize,
        out: &mut Vec<String>,
        before_metadata: &mut F,
    ) -> Result<(), String>
    where
        F: FnMut(&Dir, &OsString),
    {
        let verb = self.verb;
        if out.len() >= max_results {
            return Ok(());
        }
        let mut entries: Vec<_> = dir
            .entries()
            .map_err(|error| format!("{verb} {}: {error}", relative.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                format!(
                    "enumerate controlled {verb} {}: {error}",
                    relative.display()
                )
            })?;
        entries.sort_by_key(cap_std::fs::DirEntry::file_name);

        for entry in entries {
            if out.len() >= max_results {
                break;
            }
            let name = entry.file_name();
            let path = relative.join(&name);
            before_metadata(dir, &name);
            let before = dir.symlink_metadata(&name).map_err(|error| {
                format!(
                    "inspect controlled {verb} entry {}: {error}",
                    path.display()
                )
            })?;
            if before.file_type().is_symlink() {
                continue;
            }
            if before.is_dir() {
                if NOISE_DIRS.contains(&name.to_string_lossy().as_ref()) {
                    continue;
                }
                let expected = identity(&before);
                let child = dir.open_dir_nofollow(&name).map_err(|error| {
                    format!(
                        "open controlled {verb} directory {}: {error}",
                        path.display()
                    )
                })?;
                let opened = child.dir_metadata().map_err(|error| {
                    format!(
                        "inspect opened controlled {verb} directory {}: {error}",
                        path.display()
                    )
                })?;
                let after = dir.symlink_metadata(&name).map_err(|error| {
                    format!(
                        "revalidate controlled {verb} directory {}: {error}",
                        path.display()
                    )
                })?;
                if !opened.is_dir()
                    || after.file_type().is_symlink()
                    || !after.is_dir()
                    || identity(&opened) != expected
                    || identity(&after) != expected
                {
                    return Err(format!(
                        "controlled {verb} directory {} changed before traversal",
                        path.display()
                    ));
                }
                self.walk_controlled(&child, &path, literal, max_results, out, before_metadata)?;
                let current = dir
                    .symlink_metadata(&name)
                    .map_err(|error| format!("revalidate controlled {verb} directory: {error}"))?;
                if current.file_type().is_symlink()
                    || !current.is_dir()
                    || identity(&current) != expected
                {
                    return Err(format!(
                        "controlled {verb} directory changed during traversal"
                    ));
                }
            } else if before.is_file() {
                let entry = ControlledEntry {
                    dir,
                    name: &name,
                    path: &path,
                    before: &before,
                };
                (self.controlled_match)(&entry, literal, max_results, out)?;
            }
        }
        Ok(())
    }
}
