use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

/// Apply a context-located unified diff to one file, **atomically**. Ring 2
/// ("plan & apply structured changes"), alongside `multi_edit`. Where
/// `multi_edit` replaces the *first* occurrence of each `old_string`, an
/// `apply_patch` hunk carries surrounding **context**, so it can target a
/// *specific* occurrence — and the unified-diff format is one models emit
/// naturally. Line-based and order-preserving: hunks apply in sequence to an
/// in-memory working copy, and the file is written **once** only if every hunk
/// locates (a failure leaves the file byte-identical).
pub struct ApplyPatch;

/// One parsed hunk: the `before` block (context + removed lines) to locate, and
/// the `after` block (context + added lines) to splice in its place.
#[derive(Default)]
struct Hunk {
    before: Vec<String>,
    after: Vec<String>,
}

impl Tool for ApplyPatch {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "apply_patch".to_string(),
            description: "Apply a unified-diff patch to one file, atomically (all-or-nothing). \
                Args: {\"path\": string, \"patch\": string}. The patch is one or more hunks; each \
                hunk starts with a line beginning \"@@\" (its text is ignored — hunks are located \
                by context, not line numbers), followed by lines prefixed ' ' (context, unchanged), \
                '-' (remove), or '+' (add). Context lines disambiguate which occurrence to edit. \
                If any hunk's context cannot be located the file is left unchanged."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" },
                    "patch": {
                        "type": "string",
                        "description": "Unified-diff hunks: '@@' headers, then ' '/'-'/'+' prefixed lines"
                    }
                },
                "required": ["path", "patch"]
            }),
            permission: PermissionLevel::Write,
            ring: 2,
        }
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let patch = args
            .get("patch")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: patch".to_string())?;

        let hunks = parse_hunks(patch)?;

        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;
        let content =
            std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))?;

        // Split into lines (owned) and apply every hunk to this working copy
        // BEFORE any write. `split('\n')` + `join("\n")` round-trips the trailing
        // newline (a trailing "" element rejoins to the final "\n").
        let mut working: Vec<String> = content.split('\n').map(str::to_string).collect();
        for (i, hunk) in hunks.iter().enumerate() {
            if hunk.before.is_empty() {
                return Err(format!(
                    "hunk {i}: no context or removed lines to locate (need at least one \
                     ' ' or '-' line)"
                ));
            }
            let at = locate(&working, &hunk.before).ok_or_else(|| {
                format!("hunk {i}: context not found in {path} (the ' '/'-' lines must match)")
            })?;
            working.splice(at..at + hunk.before.len(), hunk.after.iter().cloned());
        }

        std::fs::write(&resolved, working.join("\n")).map_err(|e| format!("write {path}: {e}"))?;
        Ok(format!("applied {} hunk(s) to {path}", hunks.len()))
    }
}

/// Parse a unified-diff body into hunks. Hunk headers (`@@…`) delimit hunks and
/// are otherwise ignored (we match by context, not line numbers). Every other
/// line must be prefixed ' ' (context), '-' (remove), or '+' (add).
fn parse_hunks(patch: &str) -> Result<Vec<Hunk>, String> {
    // Drop a single trailing newline so a patch ending in "\n" doesn't yield a
    // spurious empty final line (a real blank line is encoded as " ").
    let body = patch.strip_suffix('\n').unwrap_or(patch);
    if body.is_empty() {
        return Err("patch is empty".to_string());
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current: Option<Hunk> = None;
    for line in body.split('\n') {
        if line.starts_with("@@") {
            if let Some(h) = current.take() {
                hunks.push(h);
            }
            current = Some(Hunk::default());
            continue;
        }
        let h = current
            .as_mut()
            .ok_or_else(|| "patch must start with an '@@' hunk header".to_string())?;
        match line.chars().next() {
            Some(' ') => {
                let s = line[1..].to_string();
                h.before.push(s.clone());
                h.after.push(s);
            }
            Some('-') => h.before.push(line[1..].to_string()),
            Some('+') => h.after.push(line[1..].to_string()),
            _ => {
                return Err(format!(
                    "malformed patch line (must be '@@' or start with ' ', '-', or '+'): {line:?}"
                ));
            }
        }
    }
    if let Some(h) = current.take() {
        hunks.push(h);
    }
    if hunks.is_empty() {
        return Err("patch contains no '@@' hunks".to_string());
    }
    Ok(hunks)
}

/// First index where `working[i..i+before.len()]` equals `before`.
fn locate(working: &[String], before: &[String]) -> Option<usize> {
    if before.is_empty() || before.len() > working.len() {
        return None;
    }
    (0..=working.len() - before.len()).find(|&i| working[i..i + before.len()] == before[..])
}
