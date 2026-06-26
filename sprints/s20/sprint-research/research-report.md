# Sprint 20 Research Report — Run the full agentic loop on the real backend (`bench` → openai)

> Everything to date validates **single-shot tool-call fire rate** (the toolbench).
> The product is a **multi-turn agent**; the L0–L6 ladder (`ferric bench`) tests
> real *task completion* and sets `measured_level` — the capability signal that
> promotes a model's tier and widens its rings (sprint 17's read-back). But the
> bench can't currently reach a real model on the constrained path. Fix that, then
> run it.

## The gap found (grounded in code)
`crates/ferric-bench/src/runner.rs:run_spec` spawns the child `query` with **only**:
- `--mock` (when `inv.model` is `None`), or
- `--model-dir/--model-file/--params-b/--ctx` — the **mistral GGUF** backend.

There is **no `--backend openai --api-base --model` path**. And mistral's constrained
decoding **hangs** on 0.8.15 (ADR-027), so `bench --protocol grammar` against the only
real backend it supports is a non-starter. Net: **the full-loop ladder can only run
`--mock` or mistral-unconstrained** — it has never exercised the constrained path
(the thesis) against a real model end-to-end.

Meanwhile `ferric query` already accepts `--backend openai --api-base <url> --model
<name>` (the flattened `BackendOpts` the toolbench uses). So the fix is to thread
those through the bench runner — no new query surface.

## Decisions Reviewed
- **ADR-019** — `ferric bench` is the SOLE producer of `measured_level`; this sprint doesn't change *what* it measures, only *which backend* it can measure against.
- **ADR-027** — mistral constrained hangs; the openai HTTP valve is the constrained workhorse. The bench must reach it to test the constrained loop.
- **ADR-029** (sprint 17) — `query` reads `measured_level` back. A real bench run finally populates it with non-`calibrated_ring` data, exercising that read-back end-to-end.

## Design (settled)
1. **`ferric-bench`**: add an additive `openai: Option<OpenAiArgs>` to `Invocation` (alongside the existing mistral `model: Option<ModelArgs>`); `OpenAiArgs{ api_base: Option<String>, model: String, params_b, ctx }`. `run_spec` checks `openai` first → spawns `query --backend openai [--api-base] --model <m> --params-b <p> --ctx <c> --protocol <…>`; else the mistral path; else `--mock`. Existing constructions just add `openai: None`.
2. **`bench_cmd`**: add `--backend {mistral|openai}` (+ `--api-base`, `--model`) to `BenchArgs`; build the right `Invocation` variant. Default stays mistral/mock so existing usage is unchanged.
3. **Run it**: `ferric bench --backend openai --api-base http://localhost:11434/v1 --model qwen2.5-coder:7b --params-b 7 --protocol grammar` over L0–L6 → `calibrate()` writes `measured_level` to `benchmarks/model_profiles.json`; `query --profile-dir benchmarks` then auto-applies it (ADR-029, with real data).

## Risks
- **The model may not complete all levels** — that's the *measurement*: `measured_level` = highest completed level. Partial completion is a valid, informative result.
- **Runtime** — L0–L6 multi-turn against ollama (~minutes); run in the background.
- **Refactor blast radius** — additive `openai` field keeps the mistral/mock paths + `bench --mock` test untouched; verify the `bench_mock`/`l0_smoke` tests still pass.

## Recommended approach
T-2001: openai backend in the bench runner (`Invocation.openai` + `run_spec` branch
+ `bench --backend openai/--api-base/--model`), keeping `--mock`/mistral intact;
unit/CLI coverage that the openai flags produce an openai child invocation. T-2002:
run L0–L6 against ollama qwen2.5-coder:7b → populate `measured_level`, validate the
multi-turn loop, docs + ADR + test-report. AI-verifiable: the `--mock` ladder + the
spawned-command assertion; the live run is the real-agent validation.
