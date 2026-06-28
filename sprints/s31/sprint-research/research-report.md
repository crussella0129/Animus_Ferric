# Sprint 31 Research Report — Ornstein increment 2: the `Retriever` trait + Local-FS retriever

## Sprint goal (in my words)
Ornstein is a **quarantined multi-source research subsystem** (user's expanded vision): one
funnel (the s30 quarantine = the universal sink), many pluggable **`Retriever`s**. Build the
**keystone `Retriever` trait** + the first plane — a **Local-FS retriever** — and the
orchestration that runs it source→funnel→digest end-to-end. User-chosen build order: Local FS
→ Tailnet/NAS FS → Web+container. This is the safest plane (no network), and the trait it
establishes is reused by every later plane.

## Decisions Reviewed
- **ADR-040 (s30)** — the quarantined summarizer (`summarize_quarantined`) is the sink every
  retriever feeds; this sprint adds the first source. No revision; the deferred-layers list in
  ADR-040 is the roadmap this executes against.
- **ADR-008 (deterministic output) / ADR-018 (caps)** — the retriever walk is sorted and
  capped, mirroring `search_files`.
- **ADR-005 (boundaries)** — a research root confines the walk; symlinks are not followed
  (escape safety). The *content* is still untrusted (a local file can carry an injection), so it
  goes through the quarantine regardless — that's the whole point.

## Existing Code Survey
| File | Role / relevance |
|---|---|
| `crates/ferric-tools/src/builtin/search_files.rs` | The walk pattern to reuse: recursive `read_dir`, **sorted** entries (ADR-008), `NOISE_DIRS` skip (`.git`/`target`/`node_modules`/`.ferric`), binary/unreadable skip (`read_to_string` → `Err` ⇒ skip), `max_results` cap. The retriever reuses this but returns whole matching **files** (as chunks), not match-lines. |
| `crates/ferric-tools/src/builtin/find_files.rs` | Name-match reference (the retriever matches a file by **name OR content**). |
| `crates/ferric-research/src/lib.rs` | The quarantine to feed: `summarize_quarantined(provider, source, content, question) -> ResearchDigest`. The `Retriever` trait + `research()` orchestration land in this crate. |
| `crates/ferric-provider/src/traits.rs` | `Provider` (the quarantined summarizer model); `#[async_trait]` is the pattern for the async `Retriever` trait (web/tailnet planes are genuinely async — define async now, don't break the keystone later). |
| `crates/ferric-provider/src/mock.rs` | `MockProvider` for the end-to-end `research()` test (deterministic, no live model). |

## External Sources
None — internal design; reuses the in-repo walk + quarantine.

## Risks / unknowns / dependencies
- **Trait shape is the keystone** (every plane reuses it) — surfaced at the plan/ExitPlanMode
  checkpoint for approval. Proposed: `retrieve` is **async** (FS is sync today, but web/tailnet
  are network I/O — make it async via `async-trait` now so the keystone doesn't break in inc 3/4),
  plus a sync `available()` capability probe and a `plane()` label.
- **Confinement:** walk only under the configured `root`; **don't follow symlinks**
  (`file_type().is_symlink()` ⇒ skip) to prevent escape. (Symlink *creation* needs privilege on
  Windows, so the skip is documented + covered by code, not a Windows-flaky test.)
- **Granularity:** a retriever returns candidate **documents** (whole files matching the query,
  byte-capped), each summarized into one `ResearchDigest`. Provenance `source` = the file path
  (relative to root, forward-slashed).
- **Match semantics:** case-insensitive substring on filename OR any content line (usable for a
  research query); caps on files-returned + bytes-per-file.
- **Dependency:** add `async-trait` to `ferric-research` (already a workspace dep, used by
  `ferric-provider`). No new external crate.

## Recommended approach
In `crates/ferric-research` (a new `retriever` module, re-exported):
- **Types:** `RetrievedChunk { source: String, content: String }` (raw, untrusted, with
  provenance); `RetrieveError` (thiserror).
- **`#[async_trait] trait Retriever`:** `fn plane(&self) -> &str`, `fn available(&self) -> bool`,
  `async fn retrieve(&self, query: &str) -> Result<Vec<RetrievedChunk>, RetrieveError>`.
- **`LocalFsRetriever { root, max_files, max_bytes_per_file }`:** `available()` = `root.is_dir()`;
  `retrieve` walks `root` (reusing the `search_files` pattern: sorted, skip `NOISE_DIRS`, skip
  binary, **skip symlinks**), collects files whose name or content matches (case-insensitive),
  reads content (byte-capped), `source` = relpath, caps to `max_files`.
- **`research(retriever, provider, query) -> Result<Vec<ResearchDigest>, ResearchError>`:** the
  pipeline — if `available()`, `retrieve(query)` then `summarize_quarantined` each chunk;
  collect the digests. The source→funnel→digest proof.
- **Tests (MockProvider, temp dir):** LocalFsRetriever finds a matching file (right source +
  content), excludes non-matches, skips noise dirs + binary, respects caps; `available()` false
  on a missing root; the **end-to-end `research()`** — a temp file → a `ResearchDigest` with
  `untrusted == true` and `source ==` the file (provenance), proving the first source plugs into
  the quarantine.

### Alternative considered — make `Retriever` synchronous for now
Rejected: the keystone trait is reused by the genuinely-async web/tailnet planes (inc 3/4). A
sync trait would force a breaking change then. `async-trait` is already in the workspace; pay
it once, here.
