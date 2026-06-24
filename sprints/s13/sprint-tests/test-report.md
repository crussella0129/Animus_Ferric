# Sprint 13 Test Report — Ring 0 complete + measured 100%

**Date:** 2026-06-24. The two new tools run through the registry chokepoint exactly
as the agent loop drives them; the reliability gate is a real toolbench run.

## Unit / Integration (`crates/ferric-tools/tests/builtin_file_tools.rs`) — all green
**`edit_file` (4):** replaces the first occurrence + writes back; `old_string` absent → error + file unchanged; empty `old_string` → error; outside-workspace → `Denied`.
**`delete_path` (6):** deletes a file; deletes an empty dir; non-empty dir without `recursive` → error + tree intact; with `recursive` → gone; missing path → error; outside-workspace → `Denied`; `.ferric/...` → `Denied` (denylist).
- `cargo test -p ferric-tools` 26 passed; `cargo test --workspace` green; `cargo clippy --all-targets -- -D warnings` + `cargo fmt` clean.

## E2E — the Ring-0 reliability gate (RUN, the headline)
`ferric toolbench --backend openai --api-base http://localhost:11434/v1 --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --iterations 10` over the now-complete **8-tool Ring 0** (`read_file, write_file, edit_file, list_dir, move_path, make_dir, search_files, delete_path`):

| Model | Per-tool | Overall | Verdict |
|---|---|---|---|
| qwen2.5-coder:7b | every tool 10/10 | **80/80 = 100.0%** | **solid** |
| llama3.2:1b | every tool 10/10 | **80/80 = 100.0%** | **solid** |

**Every Ring-0 tool — including the two new `edit_file` and `delete_path` — fires at
100% under the constrained path, and it holds all the way down to the 1B model.**
This is the user's "retain 100% toolcall reliability" requirement, *measured*, and it
validates the premise of the rings model: the curated core is reliable on the
smallest models, which is exactly what should be always-on (Ring 0).

## Significance for the rings north star
- The complete navigate/mutate core is empirically `solid` at 1B → it is the right
  always-on Ring 0.
- This 100% baseline is the bar the **sprint-14** ring-promotion logic will use:
  a model unlocks the next ring only once it clears `solid` on the rings inside it.
- (Note: the toolbench profile is 8.0B → Small tier, `max_tools=10`, so all 8 tools
  are benched. A *Nano*-tier run would today truncate at 6 alphabetically — the exact
  bug sprint 14's ring-aware, trim-from-outer `tools_for_policy` fixes.)

## Verdict
Ring 0 is complete, secure (zero new surface — both tools are guard-scoped `Write`),
and measured at **100% on every tool down to 1B**. No human-verification checkpoint.
