# Sprint 14 Test Report — The rings, formalized

**Date:** 2026-06-24. The ring mechanism is a compiler-checked refactor with unit
coverage for the gate + the trim; the toolbench re-confirms reliability didn't move.

## Unit (default CI — all green)
- **`ferric-core` `ring_ceiling_per_tier`** — `ring_for_tier`: Nano→0, Small→1, Medium→2, Large/Xl/Ultra→3.
- **`ferric-tools` `tools_for_policy_trims_outer_ring_first`** — 8 ring-0 + 5 ring-1 dummies, Small cap 10 → **every ring-0 (core) tool survives**, only ring-1 shed; result name-sorted (ADR-008); a Nano policy sees **only** the core ring.
- **`ferric-tools` `rings_gate_builtins_by_tier`** (real builtins) — **Nano → exactly the 6 Ring-0 core** (`write_file` present, `search_files`/`move_path` absent); **Small → all 8**. This is the bug fix made visible: no model required.
- **`tier_table_snapshot`** untouched + green (`RunPolicy` unchanged).
- `cargo test --workspace` green; `clippy --all-targets -D warnings` + `fmt` clean.

## E2E — re-confirm reliability (RUN)
`ferric toolbench --backend openai --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --iterations 10` — the 8.0B bench profile → Small → `max_ring 1` → all 8 tools (Ring 0 + Ring 1) benched:

| Model | Success | Rate | Verdict |
|---|---|---|---|
| qwen2.5-coder:7b | 80/80 | 100.0% | solid |
| llama3.2:1b | 80/80 | 100.0% | solid |

**Both rings still fire 100% on both models** — the curation + trim changed *which* tools a small model is offered (Nano now gets the surest 6, not an alphabetical 6-of-8) without touching the per-tool reliability. The grammar is now literally the active rings.

## Verdict
Rings are explicit, capability-gated, and the cap trims outside-in — the user's north
star realized, with the alphabetical-cap bug fixed and reliability re-measured at 100%.
No human-verification checkpoint. (ADR-028.)
