# Sprint 33 Test Report — Ornstein research orchestrator (ADR-043)

**Date:** 2026-06-28. Added `research_all` — run a query across all available source planes at
once, quarantine each chunk, dedup by source, and report per-plane outcomes. All tests green.

## Build / Lint (green)
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --all --check` clean.

## Unit — `ferric-research::retriever` (temp dirs + MockProvider, deterministic) — 4 new / 21 total pass
- `research_all_aggregates_across_planes`: two `LocalFsRetriever`s over different dirs → 2 digests
  (plane order); `planes` reports both `available=true`, `digests=1` each.
- **`research_all_dedups_shared_source_with_one_model_call`** (the headline): two retrievers over
  the **same** dir + a **one**-completion MockProvider → 1 digest; plane[0]=1, plane[1]=0. A
  one-completion script *passing* proves the dedup happens **before** the quarantine call (a late
  dedup would exhaust the script). Inference is spent once per unique source.
- `research_all_skips_unavailable_plane`: a missing-root plane mixed with a good one → bad plane
  `available=false`/0, good plane still contributes; no error.
- `research_all_all_unavailable_is_empty`: every plane unavailable → empty digests, all
  `available=false`.

## Verdict
**Orchestrator validated.** `research_all` delivers the multi-source payoff — one query fans out
across the live planes, results are deduped (cheaply, before inference) and aggregated, and the
`PlaneResult` report gives the observability the eventual Loop wiring needs. It composes the
existing local + tailnet planes with zero pipeline change; `research()` is untouched. The Web
plane (inc 4) remains gated on a containerizer; the remaining inc-5 work is CaMeL taint/sink-policy
+ Loop research-phase wiring. No live dependency this sprint; no human checkpoint. ADR-043.
