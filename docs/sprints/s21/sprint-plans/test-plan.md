Finalized - DO NOT EDIT

# Sprint 21 Test Plan — Fleet agentic capability map

## Regression (default CI) — the extraction must not change the single path
- `bench_mock` passes: `ferric bench --mock` runs the ladder unchanged.
- `l0_smoke` passes: the L0 mock path is byte-identical (same PASS).

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean.

## End-to-End — RUN it (the agentic capability map)
ollama up; binary built `--features backend-openai`:
```
ferric bench --backend openai --api-base http://localhost:11434/v1 \
  --models qwen2.5-coder:7b,llama3.1:8b,llama3.2:1b --params-b 7 --protocol grammar \
  --results-dir benchmarks
```
- Per model: `L0..L6 PASS/FAIL` + `calibrated <model>: measured_level N`.
- A final leaderboard: `model | measured_level | tier`, sorted by level desc.
- `benchmarks/model_profiles.json` gains one record per model with its `measured_level`.
- **Headline answered:** does `llama3.2:1b` *complete* multi-turn agentic tasks (vs its 100% single-tool-call rate)? Where does each model's loop break down? The per-model ceiling is the result — and tells us whether L7+ levels are warranted (if the fleet doesn't saturate, the ladder still discriminates).
- Whatever the levels, recorded honestly (a low measured_level is a valid measurement, not a sprint failure).

## Notes
- `bench_mock`/`l0_smoke` are the AI-verifiable core (the refactor is safe); the live fleet run is the capability map + the harder-levels decision input.
