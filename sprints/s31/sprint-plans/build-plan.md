Finalized - DO NOT EDIT

# Sprint 31 Build Plan — Ornstein increment 2: the `Retriever` trait + Local-FS retriever

Build the keystone `Retriever` trait (one funnel, many sources) + the first source plane
(Local FS), and the `research()` orchestration that runs source→funnel→digest end-to-end.
All in `ferric-research`. Rationale: `sprints/s31/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the `Retriever` keystone + Local-FS plane + the research pipeline, tested + recorded.
  - **A. trait + pipeline** — T-3101
  - **B. Local-FS retriever** — T-3102
  - **C. ADR + docs** — T-3103

## Execution Sequence

### T-3101: `Retriever` trait + `RetrievedChunk` + `RetrieveError` + `research()`
- **Touches:** `crates/ferric-research/Cargo.toml` (add `async-trait`), `crates/ferric-research/src/retriever.rs` (new), `crates/ferric-research/src/lib.rs` (mod + re-exports)
- **Depends on:** —
- **Description:** `RetrievedChunk { source, content }`; `#[async_trait] trait Retriever: Send + Sync { fn plane; fn available; async fn retrieve(query) -> Result<Vec<RetrievedChunk>, RetrieveError> }`; `RetrieveError` (thiserror); `async fn research(retriever, provider, query) -> Result<Vec<ResearchDigest>, ResearchError>` (if available → retrieve → `summarize_quarantined` each chunk → collect).
- **Success (EARS):**
  - WHEN `research` runs a retriever returning N chunks THEN it SHALL return N provenance-tagged `ResearchDigest`s (each via the quarantine).
  - WHEN the retriever is unavailable THEN `research` SHALL return an empty result, not error.

### T-3102: `LocalFsRetriever`
- **Touches:** `crates/ferric-research/src/retriever.rs`
- **Depends on:** T-3101
- **Description:** `LocalFsRetriever { root, max_files, max_bytes_per_file }`, `new(root)`. `plane()="local"`; `available()` = `root.is_dir()`; `retrieve` walks `root` (sorted, skip `NOISE_DIRS`, skip binary, **skip symlinks**), matches files by **name or content** (case-insensitive), reads byte-capped content, `source` = forward-slashed relpath, caps to `max_files`.
- **Success (EARS):**
  - WHEN a file's name or content matches THEN it SHALL be a `RetrievedChunk{source: relpath, content: capped text}`.
  - WHEN an entry is a noise dir, binary, or symlink THEN it SHALL be skipped.
  - WHEN `root` is not a directory THEN `available()` SHALL be false.

### T-3103: ADR-041 + docs
- **Touches:** `decisions.md`, `docs/ornstein.md`, `README.md`, `agent-tasks/*`
- **Depends on:** T-3102
- **Description:** ADR-041 (the `Retriever` keystone; Local-FS plane; async-keystone rationale; confinement while content stays untrusted→quarantined; the build-order ladder). `docs/ornstein.md` retriever pipeline + sample. README Status 31 + Sprint 31 timeline.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-041 and README SHALL show Sprint 31.

## Post-build (test)
- `cargo test -p ferric-research` (retriever + end-to-end research tests) + `cargo test --workspace` green; clippy `-D warnings`; fmt.
