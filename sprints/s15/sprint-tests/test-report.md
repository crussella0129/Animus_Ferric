# Sprint 15 Test Report — `--max-ring` ring override

**Date:** 2026-06-24. Fully AI-verifiable — the cap is proven end-to-end through the
real policy → `tools_for_policy` → grammar path via `--mock`; no model required.

## Unit (`ferric-tools`, default CI — green)
- **`tools_for_policy_max_ring_override_caps`** — on a Small policy (ring ceiling 1):
  - `max_ring: None` → all 8 (`policy_for` leaves the override unset).
  - `max_ring: Some(0)` → exactly the 3 ring-0 dummies (outer ring dropped).
  - `max_ring: Some(5)` (above ceiling) → all 8 (the override only lowers).

## Integration (`ferric-cli`, `--mock` — green)
- **`max_ring_caps_the_offered_tools`** — `ferric query --mock --params-b 8` (Small tier):
  - without `--max-ring` → the trace's `PromptAssembled.offered_tools` contains `search_files` + `move_path` (Ring 1).
  - `--max-ring 0` → those are **gone**, `write_file` (core) still present.
  - This asserts the cap flows **CLI → `policy.max_ring` → `tools_for_policy` → grammar** — the offered tool set *is* what the constrained grammar is built from.

## Build / Lint
- `cargo test --workspace` green (incl. the untouched `tier_table_snapshot`); `clippy --all-targets -D warnings` + `fmt` clean.

## Verdict
`--max-ring` ships: an explicit, restrict-only operator cap on the active rings —
the user's "control exactly what rings your model is using as its grammar," now a
flag. Expansion past a model's capability stays earned via `measured_level`
(ADR-019), so the knob can't footgun a weak model into rings it can't drive. No
human-verification checkpoint. (ADR-028, amended.)
