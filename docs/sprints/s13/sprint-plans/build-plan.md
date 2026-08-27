Finalized - DO NOT EDIT

# Sprint 13 Build Plan — Complete Ring 0 (`edit_file` + `delete_path`)

Fill the two Ring-0 gaps on the guard-scoped tool pattern, then measure the
complete core's toolbench fire rate (the user's "retain 100%" gate). Ring
formalization → sprint 14. Rationale: `sprints/s13/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** Ring 0 complete and measured at 100%.
  - **A. Surgical edit** — T-1301
  - **B. Delete** — T-1302
  - **C. Docs** — T-1303

## Execution Sequence

### T-1301: `edit_file` builtin
- **Touches:** `crates/ferric-tools/src/builtin/edit_file.rs` (new), `builtin/mod.rs`, `tests/builtin_file_tools.rs`
- **Success (EARS):**
  - WHEN `edit_file {path, old_string, new_string}`, **THEN** resolve `path` (`Write`), read, replace the **first** `old_string` with `new_string`, write back.
  - WHEN `old_string` empty / absent / file unreadable, **THEN** a clear error (no write).
  - `permission: Write`, `min_tier: Nano`, `target_paths` → `[path]`.
- **Notes:** mirror `write_file.rs`; first-occurrence (not require-unique) for fire rate.

### T-1302: `delete_path` builtin
- **Touches:** `crates/ferric-tools/src/builtin/delete_path.rs` (new), `builtin/mod.rs`, `tests/builtin_file_tools.rs`
- **Success (EARS):**
  - WHEN `delete_path {path, recursive?}`, **THEN** resolve (`Write`, denylist) and remove a file / empty dir; non-empty dir only with `recursive: true`, else a clear error (no deletion).
  - WHEN path missing, **THEN** a clear error.
  - `permission: Write`, `min_tier: Nano`, `target_paths` → `[path]`.
- **Notes:** mirror `move_path.rs`; non-empty-dir `recursive` gate = small-model safety; denylist already blocks `.ferric`/git/ssh.

### T-1303: Docs
- **Touches:** `README.md`, `docs/`
- **Success (EARS):** README builtin list includes `edit_file` + `delete_path`; Sprint 13 timeline entry (Ring 0 complete + measured result).

## Post-build (test)
- Integration tests (temp workspace) + the E2E toolbench reliability run → per-tool Ring-0 fire rate.
