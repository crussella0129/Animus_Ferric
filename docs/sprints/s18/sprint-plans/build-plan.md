Finalized - DO NOT EDIT

# Sprint 18 Build Plan — Round out Ring 1 (`find_files` + `copy_file`)

Widen Ring 1 ("find & organize") with the two obvious gaps: find by *name* and
copy. Two small pure-`std::fs` builtins mirroring `search_files`/`move_path`.
Small's `max_tools` (10) exactly fits Ring 0 (6) + Ring 1 (4). Rationale:
`sprints/s18/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** a coherent 4-tool Ring 1.
  - **A. find by name** — T-1801
  - **B. copy** — T-1802
  - **C. docs + re-bench** — T-1803

## Execution Sequence

### T-1801: `find_files` (Ring 1, Read)
- **New:** `crates/ferric-tools/src/builtin/find_files.rs` + register in `mod.rs`.
- **Mirror:** `search_files.rs` (sorted walk, noise-skip, cap).
- **Success (EARS):** `find_files {pattern, path?: ".", max_results?: 100}` → workspace-relative paths of files whose **name contains `pattern`**, name-sorted, capped, noise-dirs skipped; `ring: 1`, Read; empty pattern → error.
- **Tests:** finds by name; `path` scoping; cap; noise-skip.

### T-1802: `copy_file` (Ring 1, Write)
- **New:** `crates/ferric-tools/src/builtin/copy_file.rs` + register in `mod.rs`.
- **Mirror:** `move_path.rs` (both endpoints guarded).
- **Success (EARS):** `copy_file {from, to}` → `create_dir_all` parent + `std::fs::copy`; directory source → error; `ring: 1`, Write (denylist applies).
- **Tests:** copies a file; copy into `.ferric` denied; dir source errors. Bump `rings_gate_builtins_by_tier` 8 → 10.

### T-1803: Docs + re-bench
- **Touches:** `README.md`, `decisions.md`, `docs/testbench.md`.
- **Success (EARS):** README builtin list names the 4 Ring-1 tools; Sprint 18 timeline; ADR-028 sprint-18 amendment. Re-bench: `--calibrate-rings` still `solid` through Ring 1.

## Post-build (test)
- builtin units + rings-gate count (10) + the ollama `--calibrate-rings` re-bench.
