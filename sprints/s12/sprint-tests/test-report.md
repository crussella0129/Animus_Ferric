# Sprint 12 Test Report — `search_files` tool

**Date:** 2026-06-24. Fully AI-verifiable — the tool runs through the registry
execute chokepoint (its only call site), so the integration tests exercise it
exactly as the agent loop will. **No deferred E2E.**

## Integration tests (`crates/ferric-tools/tests/builtin_file_tools.rs`, temp workspace) — all green
| Test | Asserts |
|---|---|
| `search_files_finds_matches_with_relpath_and_lineno` | hits across nested dirs return `relpath:lineno:line`; results sorted (ADR-008) |
| `search_files_miss_is_empty_not_error` | absent query → empty `Ok` output, not an error |
| `search_files_caps_results` | 20 matches + `max_results:5` → exactly 5 lines (ADR-018) |
| `search_files_skips_binary_and_noise_dirs` | non-UTF-8 file skipped; a hit under `target/` skipped; the real hit returned |
| `search_files_refuses_outside_workspace` | `path:".."` → `ExecuteOutcome::Denied` via the registry/guard (ADR-005) |
| `search_files_deterministic` | two identical searches → byte-identical output (ADR-008) |

## Build / Lint
- `cargo test -p ferric-tools` — **16 passed** (6 new + 10 existing), 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt` clean.
- Default workspace tests unaffected (the new tool is additive).

## Verdict
`search_files` ships green and security-clean: every path resolves through the
`Workspace` boundary, the tool declares its target so the registry permission-checks
it, output is bounded and deterministic, and it adds no new dependency or permission.
The agent now has the find-before-edit primitive a small model needs to navigate a
codebase. No human-verification checkpoint — the whole sprint is AI-verified.
