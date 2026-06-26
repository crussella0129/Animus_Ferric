# Sprint 21 Test Report — Fleet agentic capability map (`bench --models`)

**Date:** 2026-06-26. The refactor is proven by the mock regression; the capability
map is the live fleet sweep — the most discriminating result the project has
produced.

## Regression (default CI — green)
- `bench_mock` (3 tests) + `l0_smoke` pass: extracting `run_levels` left the single-model `--mock` path **byte-identical**.
- `cargo test --workspace` green; `clippy --all-targets -D warnings` clean; `fmt --check` clean.

## End-to-End — RAN it: the agentic capability map (ollama, ConstrainedJson)
`ferric bench --backend openai --models qwen2.5-coder:7b,llama3.1:8b,llama3.2:1b --params-b 7 --protocol grammar`:
```
# Agentic Capability Leaderboard (L0-L6)
| Model            | measured_level | tier   |
|------------------|----------------|--------|
| qwen2.5-coder:7b | 6              | Large  |   ← passes all L0–L6
| llama3.1:8b      | 5              | Medium |   ← L0–L3,L5 pass; L4,L6 fail
| llama3.2:1b      | — (none)       | —      |   ← fails even L0
```

## Findings (the value)
1. **Single-tool-call reliability ≠ agentic capability.** `llama3.2:1b` fires single tool calls at **100%** (toolbench, all rings) but **cannot complete even L0** here — it took 5 turns on "list the files, then call task_complete" without a clean finish. A 1B is an excellent *constrained tool-caller* but not (yet) an autonomous multi-turn agent. This distinction was invisible until the full loop ran on the fleet.
2. **Specialization beats size for agentic coding.** The code-tuned `qwen2.5-coder:7b` (measured_level 6) **outperforms** the larger general `llama3.1:8b` (5) — which notably passes the harder L5 (mini-cli) but trips L4 (multi-file-with-test) and L6 (full-todo-app).
3. **The ladder discriminates again.** 6 / 5 / none across the fleet — so L0–L6 still separates models meaningfully; **harder levels (L7+) are a nice-to-have for ranking models *above* a 7B, not urgent.**

## Profiles persisted
`benchmarks/model_profiles.json` now has one record per model with its real
`measured_level`; `query --profile-dir` auto-applies each model's earned tier
(ADR-029). A low/absent level is a valid measurement (the sweep exits SUCCESS).

## Verdict
`bench --models` ships and produced the project's first **agentic capability map**.
The headline is an honest, important nuance: small local models can be 100%
reliable *tool-callers* well below the size at which they become reliable
*agents*. No human-verification checkpoint. (ADR-030, amended.)
