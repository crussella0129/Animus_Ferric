Finalized - DO NOT EDIT

# Sprint 12 Test Plan — `search_files` tool

## Unit / Integration (`crates/ferric-tools/tests/builtin_file_tools.rs`, temp workspace via the registry chokepoint)
- **hit:** seed files with a known marker → result contains `relpath:lineno:` per match; sorted; paths workspace-relative.
- **miss:** absent query → empty string (Ok, not an error).
- **cap:** more matches than `max_results` → exactly `max_results` lines returned.
- **binary-skip:** a non-UTF-8 file is skipped silently (no error, not in results).
- **noise-skip:** a match seeded under `target/` (or `.git/`) is NOT returned.
- **boundary (ADR-005):** `path: "../outside"` is refused through the registry/guard (mirror the existing boundary-refusal test).
- **determinism (ADR-008):** two identical runs produce byte-identical output.
- **registration:** `register_builtin_tools` exposes `search_files` (it appears in `tools_for_policy` at `Nano`).

## Build / Lint
- `cargo test -p ferric-tools` green; `cargo test --workspace` green (new tool doesn't disturb existing tests).
- `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check` clean.

## Notes
- No model needed — the registry execute chokepoint is the only call site and the tests drive it directly, so this whole sprint is AI-verifiable. No deferred E2E.
