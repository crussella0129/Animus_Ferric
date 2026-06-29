Finalized - DO NOT EDIT

# Sprint 33 Build Plan — Ornstein: the research orchestrator

Run a query across all available source planes at once: `research_all(planes, provider, query)`
probes each plane, quarantines every chunk, dedups by source (one model call per unique source),
and returns the aggregated digests + a per-plane outcome report. Pure Rust in `ferric-research`.
Rationale: `sprints/s33/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the multi-plane orchestrator, tested + recorded.
  - **A. `research_all` + report types** — T-3301
  - **B. ADR + docs** — T-3302

## Execution Sequence

### T-3301: `research_all` + `MultiResearch`/`PlaneResult`
- **Touches:** `crates/ferric-research/src/retriever.rs`, `crates/ferric-research/src/lib.rs`
- **Depends on:** —
- **Description:** `PlaneResult { plane: String, available: bool, digests: usize }`; `MultiResearch { digests: Vec<ResearchDigest>, planes: Vec<PlaneResult> }`; `async fn research_all(retrievers: &[&dyn Retriever], provider: &dyn Provider, query: &str) -> Result<MultiResearch, ResearchError>` — per retriever in order: `available()`; if available, `retrieve` then quarantine each chunk whose `source` is new (cross-plane `BTreeSet` dedup), counting; push a `PlaneResult`. Re-export the new items. `research()` untouched.
- **Success (EARS):**
  - WHEN run over several planes THEN `.digests` SHALL hold each plane's quarantined digests in plane order, and `.planes` SHALL report each plane's `available` + contributed count.
  - WHEN a `source` is surfaced by more than one plane THEN it SHALL be summarized once (later plane's count excludes it).
  - WHEN a plane is unavailable THEN it SHALL contribute nothing and be recorded `available=false`.

### T-3302: ADR-043 + docs
- **Touches:** `decisions.md`, `docs/ornstein.md`, `README.md`, `agent-tasks/*`
- **Depends on:** T-3301
- **Description:** ADR-043 (the orchestrator; chunk-level dedup; composes existing planes with zero pipeline change; Web plane still gated on Docker). docs/ornstein.md orchestrator section; README Status 33.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-043 and README SHALL show Sprint 33.

## Post-build (test)
- `cargo test -p ferric-research` (orchestrator tests + the existing 17) + `cargo test --workspace` green; clippy `-D warnings`; fmt.
