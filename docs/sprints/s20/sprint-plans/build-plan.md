Finalized - DO NOT EDIT

# Sprint 20 Build Plan — Run the full agentic loop on the real backend (`bench` → openai)

The L0–L6 ladder (`ferric bench`) tests multi-turn task completion + sets
`measured_level`, but its runner only spawns `query` with `--mock` or mistral GGUF
(which hangs under constraint, ADR-027) — so it has never run the constrained path
on a real model. Thread the openai backend through (no new `query` surface), then
run it. Rationale: `sprints/s20/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the full-loop bench reaches the constrained backend + populates real measured_level.
  - **A. openai in the bench runner** — T-2001
  - **B. run L0–L6 + docs** — T-2002

## Execution Sequence

### T-2001: openai backend in the bench runner
- **Touches:** `crates/ferric-bench/src/runner.rs` (+ lib re-export), `crates/ferric-cli/src/bench_cmd.rs`
- **Plan:** additive `openai: Option<OpenAiArgs>` on `Invocation` (`OpenAiArgs{ api_base: Option<String>, model: String, params_b: f32, ctx: u32 }`); the 2 sites (`bench_cmd.rs:107`, `Invocation::mock()`) add `openai: None`. Extract a pure `query_args(spec, inv, workspace) -> Vec<String>` from `run_spec` for testability.
- **Success (EARS):** when `inv.openai` is Some → `query --backend openai [--api-base] --model --params-b --ctx --protocol …`; else mistral (`--model-dir/--model-file`); else `--mock`. `bench --backend {mistral|openai}` (+ `--api-base`, `--model`) builds the variant; defaults unchanged.
- **Tests:** unit `query_args` for the openai Invocation (contains `--backend openai`/`--model`, not `--model-dir`); mistral/mock arms; `bench_mock`/`l0_smoke` stay green.

### T-2002: Run L0–L6 on ollama + docs
- **Touches:** `decisions.md`, `README.md`, `docs/testbench.md`, `run_benchmarks.ps1`
- **Success (EARS):** `bench --backend openai … --model qwen2.5-coder:7b --protocol grammar` over L0–L6 writes `measured_level` to `benchmarks/model_profiles.json`; ADR + docs for `bench --backend openai`; `query --profile-dir benchmarks` auto-applies the tier.

## Post-build (test)
- `query_args` units + the `--mock` ladder + the live L0–L6 run (measured_level populated; tier auto-applied).
