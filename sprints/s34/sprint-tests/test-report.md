# Sprint 34 Test Report — Ornstein CaMeL-lite sink policy (ADR-044)

**Date:** 2026-06-28. Co-designed with the user: taint tracking + a configurable sink policy on
top of the quarantine. Pure primitive, all three enforcement modes. All tests green.

## Build / Lint (green)
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --all --check` clean. `ferric-guard` added to `ferric-research` (no cycle).

## Unit — `ferric-research::sink` (pure, deterministic) — 8 new / 29 total pass
- `untainted_always_allows`: `decide(level, false)` → `Allow` for Read/Write/Execute.
- `read_sink_allows_even_tainted`: `decide(Read, true)` → `Allow` (reading isn't a dangerous sink).
- `write_execute_tainted_follow_the_mode`: `Deny`/`RequireApproval`/`Warn` mode → matching
  `SinkDecision`, for both `Write` and `Execute`.
- `taint_str_and_is_tainted` / `empty_taint_set_taints_nothing` (incl. empty/whitespace strings
  ignored on insert — no match-everything) / `taint_digest_marks_summary_and_quotes` /
  `args_tainted_walks_nested_json` (object + array nesting).
- **`end_to_end_gate_shape`** (the headline): a tainted digest's injected quote ("exfiltrate the
  api key"), echoed into `write_file` args → `args_tainted` true → `SinkPolicy::deny().decide(
  Write, true)` → `SinkDecision::Deny`. The exact gate the dispatch-chokepoint wiring will enforce.

## Verdict
**CaMeL-lite primitive validated.** The policy correctly distinguishes Read (always safe) from
Write/Execute (gated), respects all three configured modes, and the end-to-end shape test proves
it would block a real injected write. **Not yet wired into dispatch** — by design, per the user's
sprint scope. The enforcement point (`registry.execute`, beside `check(permission, path)`) and the
`TaintSet` population (as digests enter loop context) are the next increment, alongside the Web
plane (still Docker-gated). No live dependency this sprint; no human checkpoint. ADR-044.
