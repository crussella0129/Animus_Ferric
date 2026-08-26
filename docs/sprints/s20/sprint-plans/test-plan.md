Finalized - DO NOT EDIT

# Sprint 20 Test Plan — Full agentic loop on the real backend

## Unit (`ferric-bench`, default CI)
- **`query_args` openai arm:** an `Invocation` with `openai: Some(OpenAiArgs{api_base: Some("http://x/v1"), model: "qwen", params_b: 7.0, ctx: 4096})` → args contain `--backend openai`, `--api-base http://x/v1`, `--model qwen`, `--params-b 7`, and **not** `--model-dir`.
- **`query_args` mistral arm:** an `Invocation` with `model: Some(...)` (no openai) → contains `--model-dir`/`--model-file`, not `--backend openai`.
- **`query_args` mock arm:** neither → contains `--mock`.

## CLI / regression (default CI)
- `bench_mock` / `l0_smoke` still pass — `ferric bench --mock --level 0` runs the ladder unchanged (the mock path is untouched).

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean.

## End-to-End — RUN it (first multi-turn agentic run on the constrained path)
ollama up; binary built `--features backend-openai`:
```
ferric bench --backend openai --api-base http://localhost:11434/v1 \
  --model qwen2.5-coder:7b --params-b 7 --protocol grammar --results-dir benchmarks
```
- Prints per-level `L0..L6 PASS/FAIL` and `calibrated qwen2.5-coder:7b: measured_level N`.
- `benchmarks/model_profiles.json` gains a record with a real `measured_level` (highest completed level) — **not** a `--calibrate-rings` ring; the full multi-turn loop produced it.
- Then `ferric query --backend openai --api-base … --model qwen2.5-coder:7b --profile-dir benchmarks "<task>"` prints the read-back line showing the measured tier applied (ADR-029, real data).
- **Whatever level the 7B reaches is the honest result** — this is the first end-to-end validation that the constrained agentic loop completes real tasks, not just single tool calls.

## Notes
- `query_args` units + the `--mock` ladder are the AI-verifiable core; the live L0–L6 run is the real-agent validation + the measured_level data.
