# Sprint 18 Test Report — Round out Ring 1 (`find_files` + `copy_file`)

**Date:** 2026-06-25. Builtin behaviour + the rings-gate count are proven by units;
the "widening the ring kept it 100%" claim is proven by an ollama re-bench.

## Unit (`ferric-tools` — green, 31 tests)
- **`find_files_matches_names_sorted_scoped_and_skips_noise`** — `{pattern:"config"}` over a tree returns `["config.toml","src/config.rs"]` (name-sorted, excludes `notes.md` and `.git/config`); `path:"src"` scopes; `max_results:1` caps; empty pattern → error.
- **`copy_file_copies_keeps_original_and_creates_parent`** — `a.txt`→`b/a.txt` (parent made, original kept, content equal).
- **`copy_file_into_ferric_denied`** — copy into `.ferric/` → `Denied` (destination denylist).
- **`copy_file_directory_source_errors`** — a directory `from` → error (file-only).
- **`rings_gate_builtins_by_tier`** — Nano → exactly the 6 core (no Ring-1 tools incl. `find_files`/`copy_file`); Small → **10** including all four Ring-1 tools.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -D warnings` clean; `fmt --check` clean.

## End-to-End — RAN it (ollama): widening Ring 1 kept it 100%
`toolbench --calibrate-rings` for both models, Ring 1 now **10 tools** (was 8):
```
=== qwen2.5-coder:7b ===        === llama3.2:1b ===
  ring | tools |  rate | verdict   ring | tools |  rate | verdict
     0 |     6 | 100%  | solid        0 |     6 | 100%  | solid
     1 |    10 | 100%  | solid        1 |    10 | 100%  | solid
  → Recommended --max-ring 1          → Recommended --max-ring 1
```
**Both models — including the 1B — drive all 10 tools at 100%.** Adding `find_files`
+ `copy_file` to Ring 1 did **not** cost reliability: the wider grammar is still
`solid` end-to-end, which is the whole point of keeping the rings disciplined.

## Verdict
Ring 1 is now a coherent four-tool "find & organize" set (`search_files`,
`find_files`, `move_path`, `copy_file`). Two pure-`std::fs`, guard-scoped builtins
on proven templates; the unit tests pin behaviour + the rings-gate count, and the
re-bench proves growing the ring kept it 100% to 1B. No human-verification
checkpoint. (ADR-028, amended.)
