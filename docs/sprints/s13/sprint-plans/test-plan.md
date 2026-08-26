Finalized - DO NOT EDIT

# Sprint 13 Test Plan — Complete Ring 0

## Unit / Integration (`crates/ferric-tools/tests/builtin_file_tools.rs`, temp workspace via the registry chokepoint)
**`edit_file`:**
- replaces the first occurrence + writes back (read it back to confirm).
- `old_string` absent → error, file byte-unchanged.
- empty `old_string` → error.
- outside-workspace path → `ExecuteOutcome::Denied`.

**`delete_path`:**
- deletes a file (gone after).
- deletes an empty dir.
- non-empty dir without `recursive` → error, tree intact.
- non-empty dir with `recursive: true` → gone.
- missing path → error.
- outside-workspace → `Denied`; `.ferric/x` → `Denied` (denylist).

## Build / Lint
- `cargo test -p ferric-tools` + `cargo test --workspace` green.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --check` clean.

## End-to-End — the reliability gate (RUN it)
ollama already serving; the toolbench drives the now-complete 8-tool Ring 0:
```
ferric toolbench --backend openai --api-base http://localhost:11434/v1 \
  --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --iterations 10 --report ring0.md
```
- Report the **per-tool fire rate** for Ring 0, including `edit_file` + `delete_path`.
- Target **solid (100%)** on the capable models; any miss is named by the failure taxonomy (wrong_tool / malformed_args / no_action / parse_error) — that diagnosis is itself the deliverable. This is the measured "retain 100%" gate and the baseline for sprint-14 ring-promotion thresholds.
