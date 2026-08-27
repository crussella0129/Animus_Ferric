Finalized - DO NOT EDIT

# Sprint 14 Test Plan — Formalize the tool rings

## Unit (default CI)
- **`ferric-core`** `ring_for_tier` — Nano→0, Small→1, Medium→2, Large/Xl/Ultra→3.
- **`ferric-tools` registry:**
  - `tools_for_policy_trims_outer_ring_first` — register ring-0 + ring-1 tools with a `max_tools` below the total → every ring-0 tool survives, ring-1 dropped; result name-sorted.
  - existing `tools_for_policy_sorted_and_capped` still green (all-ring-0 dummies).
  - **Nano vs Small membership:** a Nano profile (`params_b < 4`) → exactly the 6 Ring-0 builtins (no alphabetical truncation); a Small profile → all 8 (Ring 0 + Ring 1).
- **`ferric-core`** `tier_table_snapshot` — untouched and green (`RunPolicy` unchanged).

## Build / Lint
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean.

## End-to-End — re-confirm (RUN it)
ollama serving; the 8.0B bench profile → Small → `max_ring 1` → all 8 tools benched:
```
ferric toolbench --backend openai --api-base http://localhost:11434/v1 \
  --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --iterations 10 --report ring1.md
```
- Confirm Ring 0 **and** Ring 1 (`search_files`, `move_path`) still fire **solid** — the curation didn't regress reliability, and the grammar is now literally the active rings.

## Notes
- The headline behaviour proof: a Nano model's grammar is now the curated 6-tool core (never the alphabetically-truncated 6-of-8) — asserted in the membership unit test, no model required.
