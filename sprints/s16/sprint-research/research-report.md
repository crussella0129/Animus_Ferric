# Sprint 16 Research Report — Ring calibration (`toolbench --calibrate-rings`)

> The user's vision: rings "expand as a model **is demonstrated to reliably call
> stuff**." s13–15 defined, measured, and made-controllable the rings; the missing
> piece is the *measurement that earns the expansion*. This sprint adds a sweep
> that benches a model ring-by-ring and reports the **highest ring it reliably
> drives** — the demonstrated-reliability promotion, operationalized.

## Decisions Reviewed
- **ADR-028** (rings) — `ring` per tool, `ring_for_tier` ceiling, `--max-ring` cap. Calibration *measures* what ceiling a model has earned.
- **ADR-019** — the bench/toolbench is the reliability instrument; `measured_level` is the earned-capability signal. Ring calibration is the same idea at ring granularity (a recommended `--max-ring`).
- **ADR-008** — deterministic, sorted output (the calibration report).

## Existing code survey (all the pieces exist)
| File | Relevance |
|------|-----------|
| `crates/ferric-cli/src/toolbench_cmd.rs` | `bench_model(...) -> BenchSummary`, `verdict(rate)` (solid≥90/marginal≥70/unreliable), `BenchSummary::overall_rate`, and `run_toolbench` (builds `policy` → `tools_for_policy` → benches). The sweep reuses all of this. |
| `RunPolicy.max_ring` + `tools_for_policy` (s14/s15) | setting `policy.max_ring = Some(r)` and re-deriving `tools_for_policy` gives the ring-`r` tool set + grammar — exactly what each sweep step benches. |
| `ferric_core::ring_for_tier` | the tier ceiling; the sweep stops once `tools_for_policy` adds no new tools (max ring reached). |

## Design (settled)
- **`--calibrate-rings`** on `toolbench`: for each model, sweep `ring_cap = 0, 1, …`:
  - `policy.max_ring = Some(ring_cap)`; recompute `all_tools = tools_for_policy(policy)` + `schema = action_schema`; `bench_model(...)` → `BenchSummary`; record `(ring_cap, verdict)`.
  - **Stop** when `tools_for_policy(ring_cap).len() == tools_for_policy(ring_cap-1).len()` (no new tools ⇒ max ring reached).
- **`recommend_max_ring(ring_solid: &[bool]) -> Option<u8>`** (pure, unit-tested): the highest ring with an unbroken `solid` prefix from 0; `None` if even ring 0 isn't solid (the model can't reliably drive the core).
- **Report:** a per-model table `ring | tools | rate | verdict`, then **"Recommended `--max-ring N`"** (or "ring 0 not solid — pick a stronger model / re-calibrate"). With `--report`, a JSONL of the per-ring rows. `--models` ⇒ one calibration block per model.
- This closes the rings loop: run it once, and it tells you the largest ring set this model earns — the demonstrated-reliability promotion the user described. (Auto-*writing* that into a persisted profile is the next, separate step; this sprint produces the recommendation operators feed to `--max-ring`/`measured_level`.)

## Risks / unknowns
- **Only rings 0–1 exist today**, so a real sweep tests 2 levels (both `solid` on capable models). That's a thin live demo but the **mechanism is forward-looking** — it exercises every ring as rings 2–3 land. The pure `recommend_max_ring` is fully unit-tested independent of model count.
- **Sweep cost** = (#rings) × (existing single bench); bounded and the user opts in via the flag.

## Recommended approach
T-1601: `recommend_max_ring` (pure + tested) + the `--calibrate-rings` sweep in `toolbench` (reuses `bench_model`/`verdict`/`max_ring`) + a per-model calibration report. T-1602: docs (the calibration workflow: "find the biggest ring your model can drive") + a `run_benchmarks.ps1` calibrate step + the Sprint 16 timeline. E2E: run it against ollama (qwen2.5-coder:7b, llama3.2:1b) → recommended ring reported.
