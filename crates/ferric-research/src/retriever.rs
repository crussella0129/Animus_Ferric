//! Ornstein research sources: the `Retriever` keystone + the Local-FS plane.
//!
//! Ornstein is "one funnel, many sources." Every [`Retriever`] returns raw,
//! UNTRUSTED chunks with provenance; [`research`] runs them all through the
//! quarantine ([`summarize_quarantined`](crate::summarize_quarantined)) into
//! typed [`ResearchDigest`]s. The funnel is source-agnostic, so each plane
//! (local FS and the web) is an additive `Retriever`, not a rewrite.

use std::collections::BTreeSet;
use std::fs::DirEntry;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ferric_provider::Provider;
use thiserror::Error;

use crate::{ResearchDigest, ResearchError, summarize_quarantined};

/// Directories never walked for research content (build / VCS noise).
const NOISE_DIRS: &[&str] = &[".git", "target", "node_modules", ".ferric"];

/// A raw, UNTRUSTED chunk retrieved from a source, carrying its provenance.
/// Goes straight to the quarantine — never to the planner.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedChunk {
    /// Provenance: where it came from (a file relpath or a URL).
    pub source: String,
    /// The untrusted raw content.
    pub content: String,
}

/// A retrieval failure.
#[derive(Debug, Error)]
pub enum RetrieveError {
    #[error("retrieve io: {0}")]
    Io(String),
    #[error("retrieve exec: {0}")]
    Exec(String),
}

/// A research **source plane**. `available()` is a runtime capability probe (a
/// network plane may be offline); `plane()` labels it. Every plane — local FS
/// and the web — implements this one trait and feeds the same
/// quarantine.
#[async_trait]
pub trait Retriever: Send + Sync {
    /// A short label for the source plane: `"local"` | `"web"`.
    fn plane(&self) -> &str;
    /// Whether this source can run right now (capability probe).
    fn available(&self) -> bool;
    /// Retrieve candidate UNTRUSTED chunks relevant to `query`.
    async fn retrieve(&self, query: &str) -> Result<Vec<RetrievedChunk>, RetrieveError>;
}

/// Run a research query against a retriever, **quarantining every chunk**. The
/// `provider` is the quarantined summarizer model. An *unavailable* retriever is
/// a no-op (empty result), not an error — a capability-probed multi-source
/// system runs only the planes that are live.
pub async fn research(
    retriever: &dyn Retriever,
    provider: &dyn Provider,
    query: &str,
) -> Result<Vec<ResearchDigest>, ResearchError> {
    if !retriever.available() {
        return Ok(Vec::new());
    }
    let chunks = retriever
        .retrieve(query)
        .await
        .map_err(|e| ResearchError::Retrieve(e.to_string()))?;
    let mut digests = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        digests.push(summarize_quarantined(provider, &chunk.source, &chunk.content, query).await?);
    }
    Ok(digests)
}

/// Per-plane outcome of an orchestrated [`research_all`] run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaneResult {
    /// The source plane label (`"local"` | `"web"`).
    pub plane: String,
    /// Whether the plane was available (capability-probed) this run.
    pub available: bool,
    /// How many (deduped) digests this plane contributed.
    pub digests: usize,
}

/// The aggregated result of running a query across several planes.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiResearch {
    /// Quarantined digests, in plane order, deduped by `source`.
    pub digests: Vec<ResearchDigest>,
    /// Per-plane outcome (what ran, what was offline, counts) — observability.
    pub planes: Vec<PlaneResult>,
}

/// Run `query` across every plane, quarantining each chunk — "one funnel, many
/// sources". Each available `Retriever` is probed, retrieved, and its chunks
/// quarantined; a `source` surfaced by more than one plane is summarized **once**
/// (deduped *before* the model call, so a duplicate costs no inference). Returns
/// the aggregated digests (plane order) plus a per-plane outcome report.
/// Unavailable planes contribute nothing and are recorded — never an error.
pub async fn research_all(
    retrievers: &[&dyn Retriever],
    provider: &dyn Provider,
    query: &str,
) -> Result<MultiResearch, ResearchError> {
    let mut digests = Vec::new();
    let mut planes = Vec::with_capacity(retrievers.len());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for retriever in retrievers {
        let available = retriever.available();
        let mut count = 0;
        if available {
            let chunks = retriever
                .retrieve(query)
                .await
                .map_err(|e| ResearchError::Retrieve(e.to_string()))?;
            for chunk in chunks {
                if seen.insert(chunk.source.clone()) {
                    digests.push(
                        summarize_quarantined(provider, &chunk.source, &chunk.content, query)
                            .await?,
                    );
                    count += 1;
                }
            }
        }
        planes.push(PlaneResult {
            plane: retriever.plane().to_string(),
            available,
            digests: count,
        });
    }
    Ok(MultiResearch { digests, planes })
}

/// The Local-FS source plane: search files under a confined `root` for a query,
/// returning matching files as untrusted chunks. Confined to `root`, symlinks
/// are not followed (escape-safety); the *content* is still untrusted (a local
/// file can carry an injection), so it goes through the quarantine like any
/// other source.
pub struct LocalFsRetriever {
    root: PathBuf,
    max_files: usize,
    max_bytes_per_file: usize,
}

impl LocalFsRetriever {
    /// Default caps: 20 files, 64 KiB each.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_caps(root, 20, 64 * 1024)
    }

    pub fn with_caps(
        root: impl Into<PathBuf>,
        max_files: usize,
        max_bytes_per_file: usize,
    ) -> Self {
        Self {
            root: root.into(),
            max_files,
            max_bytes_per_file,
        }
    }
}

#[async_trait]
impl Retriever for LocalFsRetriever {
    fn plane(&self) -> &str {
        "local"
    }

    fn available(&self) -> bool {
        self.root.is_dir()
    }

    async fn retrieve(&self, query: &str) -> Result<Vec<RetrievedChunk>, RetrieveError> {
        let terms = query_terms(query);
        let mut out = Vec::new();
        walk(
            &self.root,
            &self.root,
            &terms,
            self.max_files,
            self.max_bytes_per_file,
            &mut out,
        )?;
        Ok(out)
    }
}

/// Recurse `dir` (sorted, ADR-008), collecting files whose name or content
/// contains `needle` (already lowercased). Skips noise dirs, symlinks, and
/// binary/unreadable files. Caps the result count and each file's bytes.
fn walk(
    dir: &Path,
    root: &Path,
    terms: &[String],
    max_files: usize,
    max_bytes: usize,
    out: &mut Vec<RetrievedChunk>,
) -> Result<(), RetrieveError> {
    if out.len() >= max_files {
        return Ok(());
    }
    let mut entries: Vec<DirEntry> = std::fs::read_dir(dir)
        .map_err(|e| RetrieveError::Io(format!("{}: {e}", dir.display())))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(DirEntry::file_name);

    for entry in entries {
        if out.len() >= max_files {
            break;
        }
        // file_type() does NOT follow symlinks — so we can detect + skip them.
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if ft.is_dir() {
            if NOISE_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(&path, root, terms, max_files, max_bytes, out)?;
        } else {
            // Binary / unreadable files fall away here (non-UTF-8 → Err).
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            if matches_all_terms(&name, &content, terms) {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(RetrievedChunk {
                    source: rel,
                    content: cap_bytes(content, max_bytes),
                });
            }
        }
    }
    Ok(())
}

/// Split a research query into lowercase search terms.
///
/// The query used to be matched as ONE literal lowercase substring (ADR-078), so
/// any multi-word query — the natural way to ask for research — found nothing
/// unless that exact phrase appeared verbatim. Measured: `"configuration"`
/// matched a file, `"project notes configuration"` matched nothing in the same
/// directory, and the caller reported neither.
pub(crate) fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// A file matches when **every** term appears in its name or its content.
///
/// AND rather than OR: a research query is a conjunction of things the caller
/// wants ("tokio spawn runtime"), and OR would return most of the tree — which
/// then all goes through the quarantine model, one inference per chunk. An empty
/// term list matches nothing, so a blank query cannot sweep the workspace.
fn matches_all_terms(name: &str, content: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let name = name.to_lowercase();
    let content = content.to_lowercase();
    terms
        .iter()
        .all(|t| name.contains(t.as_str()) || content.contains(t.as_str()))
}

/// Truncate `s` to at most `max` bytes, on a char boundary.
fn cap_bytes(mut s: String, max: usize) -> String {
    s.truncate(s.floor_char_boundary(max));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_provider::{Completion, MockProvider};

    fn block<F: std::future::Future>(f: F) -> F::Output {
        futures_executor::block_on(f)
    }

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn matches_by_content_and_excludes_non_matches() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "notes.md", b"all about tailscale and NAT");
        write(dir.path(), "other.txt", b"nothing relevant here");
        let r = LocalFsRetriever::new(dir.path());
        let chunks = block(r.retrieve("tailscale")).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].source, "notes.md");
        assert!(chunks[0].content.contains("tailscale"));
    }

    #[test]
    fn matches_by_name_and_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Tailscale-setup.txt", b"body has no keyword");
        let r = LocalFsRetriever::new(dir.path());
        let chunks = block(r.retrieve("TAILSCALE")).unwrap();
        assert_eq!(chunks.len(), 1, "name match, case-insensitive");
        assert_eq!(chunks[0].source, "Tailscale-setup.txt");
    }

    #[test]
    fn skips_noise_dirs_and_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".git/config", b"tailscale in a noise dir");
        write(dir.path(), "bin.dat", &[0xff, 0xfe, 0x00, 0x01]); // non-UTF-8
        write(dir.path(), "good.txt", b"tailscale here");
        let r = LocalFsRetriever::new(dir.path());
        let chunks = block(r.retrieve("tailscale")).unwrap();
        let sources: Vec<&str> = chunks.iter().map(|c| c.source.as_str()).collect();
        assert_eq!(sources, vec!["good.txt"], "noise dir + binary skipped");
    }

    #[test]
    fn respects_max_files_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            write(dir.path(), &format!("f{i}.txt"), b"tailscale");
        }
        let r = LocalFsRetriever::with_caps(dir.path(), 2, 64 * 1024);
        let chunks = block(r.retrieve("tailscale")).unwrap();
        assert_eq!(chunks.len(), 2, "capped to max_files");
    }

    #[test]
    fn availability_and_plane() {
        let dir = tempfile::tempdir().unwrap();
        let ok = LocalFsRetriever::new(dir.path());
        assert!(ok.available());
        assert_eq!(ok.plane(), "local");
        let missing = LocalFsRetriever::new(dir.path().join("does-not-exist"));
        assert!(!missing.available());
    }

    #[test]
    fn research_pipeline_source_to_quarantined_digest() {
        // The headline: a real file on disk → a quarantined, provenance-tagged
        // digest. One matching file ⇒ one quarantine call ⇒ one digest.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "paper.md", b"tailscale enables NAT traversal");
        let r = LocalFsRetriever::new(dir.path());
        let mock = MockProvider::new(vec![Completion::assistant_text(
            r#"{"summary":"about tailscale NAT","claims":[{"claim":"NAT traversal","quote":"tailscale enables NAT traversal"}]}"#,
        )]);
        let digests = block(research(&r, &mock, "tailscale")).unwrap();
        assert_eq!(digests.len(), 1);
        assert!(
            digests[0].untrusted,
            "provenance: harness-stamped untrusted"
        );
        assert_eq!(digests[0].source, "paper.md", "provenance: the source file");
    }

    #[test]
    fn research_on_unavailable_retriever_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let r = LocalFsRetriever::new(dir.path().join("nope"));
        let mock = MockProvider::new(vec![]);
        let digests = block(research(&r, &mock, "tailscale")).unwrap();
        assert!(digests.is_empty(), "unavailable plane → no-op, not error");
    }

    #[test]
    fn research_all_aggregates_across_planes() {
        let d1 = tempfile::tempdir().unwrap();
        write(d1.path(), "a.md", b"tailscale one");
        let d2 = tempfile::tempdir().unwrap();
        write(d2.path(), "b.md", b"tailscale two");
        let r1 = LocalFsRetriever::new(d1.path());
        let r2 = LocalFsRetriever::new(d2.path());
        let mock = MockProvider::new(vec![
            Completion::assistant_text(r#"{"summary":"one","claims":[]}"#),
            Completion::assistant_text(r#"{"summary":"two","claims":[]}"#),
        ]);
        let rs: Vec<&dyn Retriever> = vec![&r1, &r2];
        let res = block(research_all(&rs, &mock, "tailscale")).unwrap();
        assert_eq!(res.digests.len(), 2, "both planes contribute");
        assert_eq!(res.planes.len(), 2);
        assert!(res.planes.iter().all(|p| p.available && p.digests == 1));
    }

    #[test]
    fn research_all_dedups_shared_source_with_one_model_call() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "shared.md", b"tailscale here");
        let r1 = LocalFsRetriever::new(dir.path());
        let r2 = LocalFsRetriever::new(dir.path()); // SAME dir → same source "shared.md"
        // Only ONE completion scripted: dedup must happen BEFORE the model call,
        // or the second plane's duplicate would exhaust the script.
        let mock = MockProvider::new(vec![Completion::assistant_text(
            r#"{"summary":"x","claims":[]}"#,
        )]);
        let rs: Vec<&dyn Retriever> = vec![&r1, &r2];
        let res = block(research_all(&rs, &mock, "tailscale")).unwrap();
        assert_eq!(res.digests.len(), 1, "shared source summarized once");
        assert_eq!(res.planes[0].digests, 1);
        assert_eq!(
            res.planes[1].digests, 0,
            "second plane's duplicate excluded"
        );
    }

    #[test]
    fn research_all_skips_unavailable_plane() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.md", b"tailscale");
        let good = LocalFsRetriever::new(dir.path());
        let bad = LocalFsRetriever::new(dir.path().join("nope")); // missing root
        let mock = MockProvider::new(vec![Completion::assistant_text(
            r#"{"summary":"x","claims":[]}"#,
        )]);
        let rs: Vec<&dyn Retriever> = vec![&bad, &good];
        let res = block(research_all(&rs, &mock, "tailscale")).unwrap();
        assert_eq!(res.digests.len(), 1);
        assert!(!res.planes[0].available && res.planes[0].digests == 0);
        assert!(res.planes[1].available && res.planes[1].digests == 1);
    }

    #[test]
    fn research_all_all_unavailable_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let bad1 = LocalFsRetriever::new(dir.path().join("nope1"));
        let bad2 = LocalFsRetriever::new(dir.path().join("nope2"));
        let mock = MockProvider::new(vec![]);
        let rs: Vec<&dyn Retriever> = vec![&bad1, &bad2];
        let res = block(research_all(&rs, &mock, "x")).unwrap();
        assert!(res.digests.is_empty());
        assert!(res.planes.iter().all(|p| !p.available && p.digests == 0));
    }
}
