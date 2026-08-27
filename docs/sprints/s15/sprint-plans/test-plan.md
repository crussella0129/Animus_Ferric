Finalized - DO NOT EDIT

# Sprint 15 Test Plan — `--max-ring` ring override

## Unit (`ferric-tools` registry, default CI)
- `tools_for_policy` with a **Small** policy (`ring_for_tier`=1, all 8 builtins admitted):
  - `max_ring: None` → all 8.
  - `max_ring: Some(1)` → all 8 (== ceiling).
  - `max_ring: Some(0)` → exactly the 6 Ring-0 core (`search_files`/`move_path` gone; `write_file` present).
  - `max_ring: Some(5)` → all 8 (above the tier ceiling ⇒ no-op cap).

## Integration (`ferric-cli`, `--mock`)
- `ferric query --mock --max-ring 0` on a Small-ish profile → the trace's `PromptAssembled.offered_tools` is **exactly the 6 core** (no `search_files`/`move_path`).
- Without `--max-ring` → all 8 offered. (Asserts the cap flows CLI → policy → `tools_for_policy` → grammar.)

## Build / Lint
- `cargo test --workspace` green (incl. the untouched `tier_table_snapshot`); `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean.

## Notes
- No model required — the offered-tools assertion runs through the real policy→grammar path via `--mock`. The reliability story is unchanged from sprint 14 (this only *restricts* the set).
