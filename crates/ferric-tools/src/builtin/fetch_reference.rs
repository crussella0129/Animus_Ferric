use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::spec::{Tool, ToolCtx, ToolSpec};

/// `fetch_reference` — on-demand retrieval from a stage's `references/` knowledge
/// vault (ADR-071; the Ferric side of Animus Dark Matter's L3 gate, see that
/// project's `INTEGRATION.md`). Returns only the top-k matching chunks as clean
/// markdown, so a stage pulls the exact slice it needs instead of pre-folding
/// whole reference files into its prompt (the token-minimality win).
///
/// This is the simple in-tree backend: recursive read of `references/`, heading
/// chunking, keyword scoring. A future Dark Matter MCP knowledge server (semantic
/// search + caching + mirrored ingestion) is a drop-in replacement behind this
/// same tool name — the model's contract does not change.
pub struct FetchReference;

const DEFAULT_K: usize = 4;
const REF_DIR: &str = "references";

impl Tool for FetchReference {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fetch_reference".to_string(),
            description: "Fetch the exact reference chunk(s) this step needs from the stage's \
                references/ knowledge vault, instead of loading whole documents. Args: \
                {\"query\": string, \"section\"?: string, \"k\"?: number}. `query` = \
                keywords/topic to search for; `section` optionally restricts to a \
                heading; `k` caps chunks returned (default 4). Returns clean-markdown \
                chunks, each headed by its ref:// source."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Keywords / topic to search the reference vault for" },
                    "section": { "type": "string", "description": "Optional: restrict to chunks whose heading contains this text" },
                    "k": { "type": "integer", "description": "Max chunks to return (default 4)" }
                },
                "required": ["query"]
            }),
            permission: PermissionLevel::Read,
            ring: 0,
        }
    }

    /// The tool only ever reads under `references/`; declare it so the registry
    /// boundary-resolves + permission-checks it before `run` (defense in depth).
    fn target_paths(&self, _args: &serde_json::Value) -> Vec<String> {
        vec![REF_DIR.to_string()]
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required string argument: query".to_string())?;
        let section = args.get("section").and_then(|v| v.as_str());
        let k = args
            .get("k")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_K)
            .max(1);

        let ref_root = ctx
            .workspace
            .resolve(REF_DIR)
            .map_err(|e| format!("boundary: {e}"))?;
        if !ref_root.is_dir() {
            return Ok(format!(
                "No reference vault for this stage (no `{REF_DIR}/` directory). Proceed without fetched references."
            ));
        }

        let mut chunks: Vec<RefChunk> = Vec::new();
        collect_chunks(&ref_root, &ref_root, &mut chunks)?;

        let terms = tokenize(query);
        let mut scored: Vec<(usize, &RefChunk)> = chunks
            .iter()
            .filter(|c| {
                section.is_none_or(|s| c.heading.to_lowercase().contains(&s.to_lowercase()))
            })
            .map(|c| (score(&terms, c), c))
            .filter(|(s, _)| *s > 0)
            .collect();
        // Highest score first; ties broken by URI for determinism (ADR-008).
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.uri.cmp(&b.1.uri)));
        scored.truncate(k);

        if scored.is_empty() {
            return Ok(format!(
                "No reference chunk matched query {query:?} (searched {} chunk(s) under `{REF_DIR}/`). Try broader keywords.",
                chunks.len()
            ));
        }

        let mut out = String::new();
        for (_, c) in &scored {
            out.push_str("### ");
            out.push_str(&c.uri);
            out.push('\n');
            out.push_str(c.text.trim());
            out.push_str("\n\n");
        }
        Ok(out.trim_end().to_string())
    }
}

/// One retrievable slice of a reference file.
struct RefChunk {
    /// `ref://<path-relative-to-references>#<index>`.
    uri: String,
    /// The chunk's heading text (empty for a heading-less chunk), for `section`.
    heading: String,
    /// The chunk body (includes its heading line).
    text: String,
}

/// Recursively gather chunks from every text file under `dir`, deterministically
/// (sorted paths). `root` is `references/` so URIs are relative to it.
fn collect_chunks(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<RefChunk>,
) -> Result<(), String> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_chunks(root, &path, out)?;
            continue;
        }
        let is_text = path
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| {
                x.eq_ignore_ascii_case("md")
                    || x.eq_ignore_ascii_case("markdown")
                    || x.eq_ignore_ascii_case("txt")
                    || x.eq_ignore_ascii_case("mdx")
            })
            .unwrap_or(false);
        if !is_text {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue; // skip unreadable/non-UTF8 files rather than fail the fetch
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for (i, (heading, body)) in chunk_markdown(&content).into_iter().enumerate() {
            out.push(RefChunk {
                uri: format!("ref://{rel}#{i}"),
                heading,
                text: body,
            });
        }
    }
    Ok(())
}

/// Split markdown into `(heading, body)` chunks at ATX headings (`#`…). A
/// heading-less file yields a single chunk.
fn chunk_markdown(content: &str) -> Vec<(String, String)> {
    let mut chunks: Vec<(String, String)> = Vec::new();
    let mut heading = String::new();
    let mut body = String::new();
    let mut started = false;
    for line in content.lines() {
        if line.trim_start().starts_with('#') {
            if started && !body.trim().is_empty() {
                chunks.push((heading.clone(), std::mem::take(&mut body)));
            }
            heading = line.trim().trim_start_matches('#').trim().to_string();
            body.push_str(line);
            body.push('\n');
            started = true;
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !body.trim().is_empty() {
        chunks.push((heading, body));
    }
    if chunks.is_empty() && !content.trim().is_empty() {
        chunks.push((String::new(), content.to_string()));
    }
    chunks
}

/// Shortest query term matched as a substring. At or below this length a
/// substring match is mostly noise — `"go"` occurs inside `"algorithm"` — so
/// short terms are matched as whole words instead of being discarded (ADR-073).
const MIN_SUBSTRING_TERM_CHARS: usize = 3;

/// Lowercase alphanumeric tokens. Short tokens are KEPT: dropping them made
/// `"Go"`, `"AI"`, `"C"` and `"k8"` return nothing at all over a vault full of
/// matches. They are scored differently rather than thrown away — see `score`.
fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// Count how many distinct query terms appear in the chunk (heading + body).
///
/// Longer terms match as substrings, which is what lets `"concurren"` find
/// "concurrency". Short terms (< [`MIN_SUBSTRING_TERM_CHARS`]) must match a
/// whole word, so `"go"` finds the language and not `"algorithm"`.
fn score(terms: &[String], c: &RefChunk) -> usize {
    let hay = format!("{} {}", c.heading, c.text).to_lowercase();
    let words: Vec<&str> = hay
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();

    terms
        .iter()
        .filter(|t| {
            if t.chars().count() < MIN_SUBSTRING_TERM_CHARS {
                words.contains(&t.as_str())
            } else {
                hay.contains(t.as_str())
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_guard::Workspace;
    use serde_json::json;

    fn ws_with_refs(files: &[(&str, &str)]) -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let refs = dir.path().join(REF_DIR);
        std::fs::create_dir_all(&refs).unwrap();
        for (name, body) in files {
            let p = refs.join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, body).unwrap();
        }
        let ws = Workspace::new(dir.path()).unwrap();
        (dir, ws)
    }

    fn run(ws: &Workspace, args: serde_json::Value) -> Result<String, String> {
        FetchReference.run(&ToolCtx { workspace: ws }, &args)
    }

    #[test]
    fn spec_is_read_and_names_query() {
        let s = FetchReference.spec();
        assert_eq!(s.name, "fetch_reference");
        assert_eq!(s.permission, PermissionLevel::Read);
        assert!(
            s.input_schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("query"))
        );
    }

    #[test]
    fn target_path_is_references_dir() {
        // WHEN the guard inspects the call THEN the only declared target SHALL be references/.
        assert_eq!(FetchReference.target_paths(&json!({})), vec!["references"]);
    }

    #[test]
    fn returns_only_matching_chunk() {
        // WHEN query matches one heading's chunk THEN only that chunk SHALL be returned.
        let (_d, ws) = ws_with_refs(&[(
            "api.md",
            "# Tokio spawn\nUse tokio::spawn to run a future.\n\n# Serde derive\nUse #[derive(Serialize)].\n",
        )]);
        let out = run(&ws, json!({"query": "tokio spawn"})).unwrap();
        assert!(out.contains("tokio::spawn"), "got: {out}");
        assert!(
            !out.contains("Serialize"),
            "must not include the unrelated chunk: {out}"
        );
        assert!(
            out.contains("ref://api.md#"),
            "chunk must carry a ref:// uri: {out}"
        );
    }

    #[test]
    fn k_caps_results() {
        // WHEN k=1 and several chunks match THEN at most one chunk SHALL be returned.
        let (_d, ws) = ws_with_refs(&[(
            "a.md",
            "# one alpha\nalpha\n# two alpha\nalpha\n# three alpha\nalpha\n",
        )]);
        let out = run(&ws, json!({"query": "alpha", "k": 1})).unwrap();
        assert_eq!(out.matches("ref://").count(), 1, "k=1 → one chunk: {out}");
    }

    #[test]
    fn section_filter_restricts_to_heading() {
        let (_d, ws) = ws_with_refs(&[(
            "a.md",
            "# Runtime\nspawn tasks here\n# Errors\nspawn may fail\n",
        )]);
        let out = run(&ws, json!({"query": "spawn", "section": "Errors"})).unwrap();
        assert!(out.contains("spawn may fail"));
        assert!(
            !out.contains("spawn tasks here"),
            "section filter should exclude Runtime: {out}"
        );
    }

    #[test]
    fn no_references_dir_is_graceful() {
        // WHEN the stage has no references/ THEN the tool SHALL return a note, not an error.
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        let out = run(&ws, json!({"query": "anything"})).unwrap();
        assert!(out.contains("No reference vault"), "got: {out}");
    }

    #[test]
    fn no_match_reports_cleanly() {
        let (_d, ws) = ws_with_refs(&[("a.md", "# Topic\nsome content\n")]);
        let out = run(&ws, json!({"query": "nonexistentzzz"})).unwrap();
        assert!(out.contains("No reference chunk matched"), "got: {out}");
    }

    #[test]
    fn missing_query_is_error() {
        let (_d, ws) = ws_with_refs(&[("a.md", "# T\nx\n")]);
        assert!(run(&ws, json!({})).is_err());
    }

    // --- ADR-073: short-token queries ---

    #[test]
    fn short_token_query_matches() {
        // Regression: `tokenize` dropped tokens of <= 2 chars, so "Go" produced
        // an empty term list, every chunk scored 0, and the tool reported "no
        // match" over a vault entirely about Go.
        let (_d, ws) = ws_with_refs(&[(
            "langs.md",
            "# Go concurrency\n\nGo uses goroutines and channels.\n",
        )]);
        for q in ["Go", "go"] {
            let out = run(&ws, json!({ "query": q })).unwrap();
            assert!(
                out.contains("Go concurrency"),
                "query {q:?} must match a vault about Go, got: {out}"
            );
        }
    }

    #[test]
    fn short_token_matches_whole_words_only() {
        // Why short tokens were dropped in the first place: substring matching
        // makes "go" hit "algorithm". Word matching keeps them usable without
        // reintroducing that noise.
        let (_d, ws) = ws_with_refs(&[(
            "algo.md",
            "# Sorting\n\nThis algorithm is a mergesort. Nothing else here.\n",
        )]);
        let out = run(&ws, json!({ "query": "Go" })).unwrap();
        assert!(
            out.starts_with("No reference chunk matched"),
            "\"Go\" must not match \"algorithm\", got: {out}"
        );
    }

    #[test]
    fn longer_terms_still_match_as_substrings() {
        // Stem matching is load-bearing for normal queries and must survive.
        let (_d, ws) = ws_with_refs(&[("r.md", "# Runtime\n\nconcurrency model\n")]);
        let out = run(&ws, json!({ "query": "concurren" })).unwrap();
        assert!(out.contains("Runtime"), "got: {out}");
    }
}
