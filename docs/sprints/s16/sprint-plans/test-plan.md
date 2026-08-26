Finalized - DO NOT EDIT

# Sprint 16 Test Plan — Ring calibration

## Unit (`ferric-cli`, gated `backend-openai`/`test`)
- `recommend_max_ring`:
  - `[true, true]` → `Some(1)` (solid through ring 1).
  - `[true, false]` → `Some(0)` (ring 1 not solid → cap at 0).
  - `[false, ..]` → `None` (even ring 0 not solid).
  - `[true, true, true]` → `Some(2)` (proves it scales past today's 2 rings).

## Build / Lint
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean. The sweep is gated behind the backend features (like the rest of `toolbench`).

## End-to-End — RUN it
ollama serving; sweep both models across the rings present (0–1):
```
ferric toolbench --backend openai --api-base http://localhost:11434/v1 \
  --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --calibrate-rings --report calib.md
```
- Each model prints `ring | tools | rate | verdict` for ring 0 (6 core) and ring 1 (8 tools), then **"Recommended --max-ring 1"** (both solid through ring 1).
- This is the headline artifact: a single command that reports the largest ring a model has *earned*.

## Notes
- The `recommend_max_ring` unit test proves the recommendation logic across more rings than exist today, so the command is correct ahead of rings 2–3 landing.
