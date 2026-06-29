# Plan Critique — Sprint 33

> Self-critique against `prompts/plan-critic.md` (user-steered; no subagent spawn).

## Concerns

### C-001: Is `research_all` too thin to be a sprint?
- **Failure mode:** trivial-increment
- **Response:** it's small but it's the *payoff* of the multi-source design + it carries real
  decisions: **chunk-level dedup** (a source from two planes costs one model call, not two — the
  expensive resource), a **per-plane outcome report** (the observability the Loop wiring needs),
  and deterministic plane-ordered aggregation. The dedup test (1-completion script for a shared
  source) proves a non-obvious property. Worth its own clean increment; the heavier inc 4/inc-5
  pieces are blocked (Docker) or want the user (CaMeL).

### C-002: Dedup after the model call would waste calls
- **Failure mode:** wasted-compute
- **Response:** dedup is at the **chunk `source`** level *before* `summarize_quarantined`, so a
  duplicate never reaches the model. The test asserts this structurally: a shared source with a
  **one**-completion MockProvider script must pass (two completions would mean dedup ran too late).

### C-003: Per-plane credit for a deduped source
- **Failure mode:** ambiguous-accounting
- **Response:** deterministic by plane order — the first plane to surface a `source` gets the
  count; later planes' duplicates are excluded (count 0 for that source). Documented + tested.

### C-004: `research()` regression risk
- **Failure mode:** churn
- **Response:** `research()` (single-plane) is **untouched**; `research_all` is additive. The
  existing 17 crate tests must stay green (verified by `cargo test -p ferric-research`).

### C-005: `&[&dyn Retriever]` ergonomics
- **Failure mode:** awkward-API
- **Response:** callers build a `Vec<&dyn Retriever>` of mixed concrete retrievers — fine for the
  eventual Loop-wiring caller and the tests. A more ergonomic builder can come with the Loop wiring
  if needed.

## Confidence
`clean` — a small, additive, fully-deterministic composition of already-tested planes, with the
two non-obvious properties (pre-quarantine dedup, per-plane reporting) each pinned by a test. No
Docker/network/loop dependency; `research()` untouched.
