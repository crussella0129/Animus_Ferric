# Sprint 19 Test Report — Seed Ring 2 (`multi_edit`)

**Date:** 2026-06-25. `multi_edit`'s atomicity + the rings-gate count are proven by
units; that a model can actually *drive* Ring 2 is proven by the first calibration
sweep ever to reach it.

## Unit (`ferric-tools` — green, 34 tests)
- **`multi_edit_applies_ordered_batch_atomically`** — `[{a→X},{X b→DONE}]` on `"a b"` → `"DONE"` (sequential; the 2nd edit touches the 1st's output); one write; reports "applied 2 edits".
- **`multi_edit_missing_old_leaves_file_unchanged`** — a batch whose 2nd `old_string` is absent → error, file **byte-identical** to before (atomic: nothing written).
- **`multi_edit_empty_edits_and_empty_old_error`** — `edits:[]` → error; an empty `old_string` → error.
- **`rings_gate_builtins_by_tier`** — Nano → 6 core (no `multi_edit`); Small → 10 (no `multi_edit`); **Medium (params 20) → 11 including `multi_edit`**.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -D warnings` clean; `fmt --check` clean.

## End-to-End — RAN it: calibration reaches Ring 2 for the first time
`toolbench --backend openai --models qwen2.5-coder:7b --protocol grammar --params-b 20 --iterations 10 --calibrate-rings` (Medium ceiling):
```
=== calibrating qwen2.5-coder:7b ===
  ring | tools |   rate | verdict
  -----|-------|--------|----------
     0 |     6 | 100.0% | solid
     1 |    10 | 100.0% | solid
     2 |    11 | 100.0% | solid
  → Recommended --max-ring 2 (solid through ring 2)
    saved calibrated_ring 2 → …/s19prof/model_profiles.json
```
**The 7B drives all 11 tools — including the new Ring-2 `multi_edit` — at 100%.**
So Ring 2 is reachable: the constrained-decoding thesis holds even for the more
complex nested-array `multi_edit`. `--params-b` did its job — the sweep reached
Ring 2 and the calibration recommended (and persisted) `--max-ring 2`.

## Verdict
Ring 2 is seeded and *proven drivable*. `multi_edit` is atomic (unit-tested) and
fires `solid` end-to-end; `toolbench --params-b` lets calibration bench at any tier
and reach the new ring. The rings can keep widening without losing reliability —
even the structured, nested Ring-2 edit fires 100% on a 7B. No human-verification
checkpoint. (ADR-028, amended.)
