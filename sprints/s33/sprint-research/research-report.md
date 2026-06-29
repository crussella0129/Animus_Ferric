# Sprint 33 Research Report — the research orchestrator (run a query across all planes)

## Sprint goal (in my words)
We now have two source planes behind the `Retriever` keystone (Local-FS s31, Tailnet/NAS s32),
each runnable via `research(one_retriever, provider, query)`. The payoff of "one funnel, many
sources" is running a query across **all available planes at once**. Build the **research
orchestrator** — `research_all(planes, provider, query)` — which capability-probes each plane,
quarantines every chunk, **dedups by source** (a file reachable from two planes is summarized
once), and returns the aggregated digests **plus a per-plane outcome report** (what ran, how
many each gave). Pure Rust, in `ferric-research`; no Docker/network dependency.

**Why this and not inc 4 (Web+container) or inc 5's CaMeL sink-policy:** inc 4 is blocked (no
containerizer installed; probed again this session — `docker` absent on Windows + WSL). The
CaMeL taint/sink-policy is the more cross-cutting piece (it wires taint into the loop's tool
dispatch) and is better with the user present + the web plane existing. The orchestrator is the
self-contained, buildable-now completion of the multi-source vision.

## Decisions Reviewed
- **ADR-040/041/042** — the quarantine + the `Retriever` keystone + the local/tailnet planes.
  The orchestrator composes them; no revision. `research()` already encapsulates "probe →
  retrieve → quarantine each chunk"; `research_all` is the multi-plane, dedup-aware sibling.
- **ADR-008 (deterministic output)** — aggregation is in plane order; dedup keeps the first
  occurrence; the per-plane report is deterministic.

## Existing Code Survey
| File | Role / relevance |
|---|---|
| `crates/ferric-research/src/retriever.rs` | `Retriever{plane,available,retrieve}`, `RetrievedChunk{source,content}`, `research(retriever,provider,query)` (probe → quarantine each chunk → `Vec<ResearchDigest>`), `LocalFsRetriever`, `TailnetFsRetriever`. The orchestrator runs the same probe→retrieve→quarantine per plane with cross-plane dedup + reporting. |
| `crates/ferric-research/src/lib.rs` | `summarize_quarantined` (the quarantine); re-exports — add the orchestrator types. |
| `crates/ferric-provider/src/mock.rs` | `MockProvider` for deterministic multi-plane tests (script one completion per unique chunk). |

## External Sources
None — internal composition of existing pieces.

## Risks / unknowns / dependencies
- **Dedup point:** dedup at the **chunk `source`** level *before* the quarantine call, so a
  source surfaced by two planes costs only one model call (model calls are the expensive part).
  Per-plane `digests` counts the *unique* sources that plane first contributed (deterministic by
  plane order).
- **Availability is per-plane:** an unavailable plane (offline tailnet, missing root) contributes
  nothing and is recorded `available=false` — the orchestrator runs the live planes (the existing
  `research()` no-op-on-unavailable behavior, made visible in the report).
- **`&[&dyn Retriever]` ergonomics:** callers build a `Vec<&dyn Retriever>` of mixed concrete
  retrievers; fine for the loop-wiring caller + tests.
- **No global cap this increment:** per-retriever caps already bound each plane; a cross-plane
  total cap is a trivial later addition if needed.

## Recommended approach
In `crates/ferric-research/src/retriever.rs`:
- `PlaneResult { plane: String, available: bool, digests: usize }` and
  `MultiResearch { digests: Vec<ResearchDigest>, planes: Vec<PlaneResult> }`.
- `async fn research_all(retrievers: &[&dyn Retriever], provider: &dyn Provider, query: &str) ->
  Result<MultiResearch, ResearchError>`: for each retriever in order — probe `available()`; if
  available, `retrieve(query)` then for each chunk whose `source` is new (cross-plane
  `BTreeSet` dedup) `summarize_quarantined` it and count; push a `PlaneResult`. Return the
  aggregated, deduped, plane-ordered `MultiResearch`.
- Re-export `research_all`, `MultiResearch`, `PlaneResult` from `lib.rs`.
- **Tests (temp dirs + `MockProvider`):** two `LocalFsRetriever`s over different dirs → digests
  from both, both planes `available` with right counts; an unavailable plane (missing root) →
  `available=false`, 0 digests, others still contribute; **dedup** — two retrievers over the
  *same* dir → the shared source appears **once**, the second plane's count is 0; aggregation is
  in plane order.

### Alternative considered — return a flat `Vec<ResearchDigest>` (no report)
Rejected: the per-plane `PlaneResult` (what ran, what was offline, counts) is exactly the
observability the eventual Loop research-phase wiring + the user need ("searched local + tailnet;
tailnet offline; 3 digests"). It's a few lines and deterministic.
