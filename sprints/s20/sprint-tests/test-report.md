# Sprint 20 Test Report — Full agentic loop on the real backend (`bench` → openai)

**Date:** 2026-06-26. The wiring is proven by `query_args` units + the `--mock`
ladder; the multi-turn loop is proven by the first real L0–L6 run on the
constrained path. Running it also **surfaced and fixed a verification bug**.

## Unit (`ferric-bench` — green, 20 tests)
- **`query_args_openai_arm_targets_the_valve`** — an `openai` Invocation → `--backend openai`, `--api-base`, `--model`, `--protocol grammar`; no `--model-dir`/`--mock`.
- **`query_args_mistral_arm_unchanged`** / **`query_args_mock_arm`** — the GGUF and mock paths are byte-identical to before.
- Existing `bench_mock` / `l0_smoke` still pass.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -D warnings` clean; `fmt --check` clean.

## Bug found by running it (fixed)
First real run: every level FAILED with `tools_ok: false`, `tools_called: ['list_dir']` — yet `terminator: task_complete`. `task_complete` is a structured *terminator* (SessionEnd), not a dispatched ToolCall, so `parse_trace` never credited it, and no spec's `expected_tools=["task_complete"]` could ever verify. **Fix:** `parse_trace` now credits the `task_complete` terminator as a called tool (`verify.rs`). The loop's tracing was correct; the bench's accounting was not.

## End-to-End — RAN it: the full agentic loop on the constrained path
`ferric bench --backend openai --api-base http://localhost:11434/v1 --model qwen2.5-coder:7b --params-b 7 --protocol grammar` over L0–L6:
```
L0 single-readonly-tool    — PASS (2 turns,   70 tok)
L1 single-file-rename      — PASS (2 turns,   69 tok)
L2 multi-step-ops          — PASS (4 turns,  126 tok)
L3 single-file-construction— PASS (2 turns,   87 tok)
L4 multi-file-with-test    — PASS (3 turns,  143 tok)
L5 mini-cli                — PASS (3 turns,  380 tok)
L6 full-todo-app           — PASS (5 turns, 1110 tok)
calibrated qwen2.5-coder:7b: measured_level 6 (Small -> Large)
```
- **All 7 levels pass** — the multi-turn constrained loop completes every task, from a single readonly call up to a full todo app (L6, 5 turns).
- `benchmarks/model_profiles.json` now carries a **real** `measured_level: 6` — the first non-`calibrate-rings` profile data. A 7B "Small" by params proves "**Large**" by demonstrated capability (the ADR-019 bidirectional override in action).
- **Read-back closes the loop:** `query --model qwen2.5-coder:7b --profile-dir benchmarks` prints `profile qwen2.5-coder:7b: measured_level Some(6)` and applies the Large tier (ADR-029, now with real bench data).

## Verdict
The full agentic loop is validated end-to-end on the constrained backend — not just
single tool-call fire rate but real multi-turn *task completion*. The bench now
reaches the constrained workhorse (ollama), a verification bug is fixed, and the
demonstrated-reliability promotion runs on real data: bench → `measured_level 6` →
`query` auto-promotes to Large. No human-verification checkpoint. (ADR-030.)
