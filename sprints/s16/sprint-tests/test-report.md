# Sprint 16 Test Report — Ring calibration (`toolbench --calibrate-rings`)

**Date:** 2026-06-24. Both the pure recommendation logic (unit, no model) and the
full sweep (E2E vs ollama) are verified.

## Unit (`ferric-cli` — green)
- **`recommend_max_ring_longest_solid_prefix`** — `[true,true]`→`Some(1)`; `[true,false]`→`Some(0)`; `[false,_]`→`None`; `[]`→`None`; `[true,true,true]`→`Some(2)`; `[true,true,false,true]`→`Some(1)` (a solid ring *after* a break doesn't count — unbroken prefix only). Proves the logic across more rings than exist today.

## Build / Lint
- `cargo build -p ferric-cli --features backend-openai` clean (the calibrate branch compiles); `cargo clippy -p ferric-cli --features backend-openai --all-targets -D warnings` clean; `cargo fmt` clean.

## End-to-End — the headline (RAN it, ollama)
`ferric toolbench --backend openai --api-base http://localhost:11434/v1 --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --iterations 10 --calibrate-rings --report calib.md`:

```
=== calibrating qwen2.5-coder:7b ===
  ring | tools |   rate | verdict
  -----|-------|--------|----------
     0 |     6 | 100.0% | solid
     1 |     8 | 100.0% | solid
  → Recommended --max-ring 1 (solid through ring 1)

=== calibrating llama3.2:1b ===
  ring | tools |   rate | verdict
  -----|-------|--------|----------
     0 |     6 | 100.0% | solid
     1 |     8 | 100.0% | solid
  → Recommended --max-ring 1 (solid through ring 1)
```

- The sweep benched ring 0 (6 core), then ring 1 (8 tools), then **stopped** (ring 2 added no tools) — the auto-detected max-ring termination works.
- **Both models — including the 1B — calibrate to `--max-ring 1`** at 100%. Every ring present is solid, so the recommendation is the top ring, exactly as the recommendation logic predicts.
- `calib.md` + `calib.jsonl` written (the per-ring rows, model-tagged).

## Verdict
Ring calibration ships: one command reports the largest ring a model has *earned*
(the recommended `--max-ring`). It closes the rings loop — "rings expand as a model
is demonstrated to reliably call stuff" is now a measurement, not a manual sweep.
The 2-ring demo is bounded by today's tool set; the sweep + `recommend_max_ring`
already handle rings 2–3 the moment they land (proven by the unit test). No
human-verification checkpoint. (ADR-028.)
