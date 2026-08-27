Finalized - DO NOT EDIT

# Sprint 17 Test Plan — Durable promotion (profile read-back)

## Unit (`ferric-bench`, default CI)
- `read_profile` round-trips a written record (read-after-`write_profile` == the record).
- `read_profile` on a missing file → `None`; on a present file but absent (model,protocol) → `None`.
- `write_calibrated_ring` on a record that already has `measured_level: Some(4)` sets `calibrated_ring` **without** clearing `measured_level`.
- Old JSON (a record serialized before the field existed) deserializes with `calibrated_ring: None` (serde default).

## Integration (`ferric-cli`, `--mock`)
- **Read-back applies:** write `model_profiles.json` into a temp dir with the mock model's (model, protocol) key carrying `calibrated_ring: 0` (and/or `measured_level`). `ferric query --mock --params-b 8 --profile-dir <tmp>` → the trace's `PromptAssembled.offered_tools` is the Ring-0 core (no `search_files`/`move_path`), proving the persisted ring reached the grammar.
- **No-op safety:** the same `query --mock --params-b 8` with **no** `--profile-dir` file → offered set unchanged from today (still contains `search_files`). (Pins that the feature can't silently alter un-calibrated runs.)

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean.

## End-to-End — RUN it
- `ferric toolbench --backend openai --api-base http://localhost:11434/v1 --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --iterations 10 --calibrate-rings --profile-dir benchmarks` → `benchmarks/model_profiles.json` shows `calibrated_ring: 1` for both models (`benchmarks/` is gitignored).
- Then `ferric query --backend openai --api-base … --model llama3.2:1b --profile-dir benchmarks "<task>"` auto-applies `--max-ring 1` from the profile (visible in the trace), no manual flag.

## Notes
- The `--mock` test is the AI-verifiable core (no model); the ollama E2E demonstrates the write→read handoff with real models.
