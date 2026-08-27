Finalized - DO NOT EDIT

# Sprint 19 Test Plan — Seed Ring 2 (`multi_edit`)

## Unit (`ferric-tools`, default CI)
- **`multi_edit` atomic batch:** `multi_edit {path, edits:[{old:"a",new:"X"},{old:"X b",new:"DONE"}]}` on `"a b"` → `"DONE"` (sequential: the 2nd edit touches text the 1st inserted); one write.
- **`multi_edit` aborts with nothing written:** a batch where the 2nd `old_string` is absent → error, and the file is **byte-identical to before** (no partial write).
- **`multi_edit` empties error:** `edits: []` → error; an edit with empty `old_string` → error.
- **`rings_gate_builtins_by_tier`:** Nano → 6 core (no `multi_edit`); Small (params 8) → 10 (no `multi_edit`); **Medium (params 20) → 11 including `multi_edit`**.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean.

## End-to-End — RUN it (first time calibration reaches Ring 2)
- `ferric toolbench --backend openai --api-base http://localhost:11434/v1 --models qwen2.5-coder:7b --protocol grammar --iterations 10 --params-b 20 --calibrate-rings` → sweeps **rings 0, 1, 2** (Medium ceiling). Records the per-ring verdict for ring 2 (the `multi_edit`-bearing ring): `solid` ⇒ Ring 2 is reachable by a 7B; `marginal`/`unreliable` ⇒ calibration correctly recommends `--max-ring 1` (the demonstrated-reliability gate doing its job). **Either result is valid and recorded honestly** — this is a measurement, not a pass/fail.

## Notes
- The unit tests (atomicity + gate count) are the AI-verifiable core; the `--params-b 20` sweep is the live measurement of whether the new ring fires.
