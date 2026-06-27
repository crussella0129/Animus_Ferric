Finalized - DO NOT EDIT

# Sprint 29 Test Plan — `apply_patch` (round out Ring 2)

## Integration — `builtin_file_tools.rs` (`ferric-tools`, via `setup()` → temp ws + registry)
- **single hunk applies:** a file `a\nb\nc`; a hunk ` a\n-b\n+B\n c` → file becomes `a\nB\nc` (exact content asserted); one write; reports success.
- **the defining contrast (vs `multi_edit`):** a file with two identical `x` lines; a hunk whose **context** pins the *second* (`-x\n+X` with the preceding unique line as context) edits the **second** `x` — where `multi_edit`'s first-occurrence rule would hit the first. This is the capability `multi_edit` lacks.
- **unlocatable hunk → error + no write:** a hunk whose `before` block is absent → `Err`, and the file is **byte-identical** to before (read it back and compare).
- **malformed/empty patch → error:** an empty `patch` string → `Err`; a body line lacking a ` `/`-`/`+ prefix → `Err`; both leave the file untouched.
- **multi-hunk in order:** two hunks in one patch both apply (the 2nd may touch context the 1st produced); final content asserted.

## Ring-gate — `rings_gate_builtins_by_tier`
- **Medium == 12** (Ring 0 `6` + Ring 1 `4` + `multi_edit` + `apply_patch`), and `medium_names` contains `apply_patch`.
- **Nano still 6**, **Small still 10** — `apply_patch` (ring 2) absent below Medium.

## Build / Lint (default CI)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean. No registry/scale change (Medium `max_tools=16` ≥ 12, no trimming).

## E2E
- Not required: a pure-`std::fs` builtin is fully exercised through the registry in the integration tests (the same granularity that covers `multi_edit`/`edit_file`). A live calibration run driving `apply_patch` under a real model is future work — Ring 2 is already proven drivable (`multi_edit`, qwen-7b `--max-ring 2` 100%).
