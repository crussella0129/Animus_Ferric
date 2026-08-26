Finalized - DO NOT EDIT

# Sprint 12 Build Plan — A workspace `search_files` tool

Add the missing content-search primitive a small coding agent needs most.
Guard-scoped, dependency-free, mirrors `list_dir`. Rationale:
`sprints/s12/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the agent can find text across the workspace before acting.
  - **A. The tool** — T-1201
  - **B. Docs** — T-1202

## Execution Sequence

### T-1201: `SearchFiles` builtin tool
- **Touches:** `crates/ferric-tools/src/builtin/search_files.rs` (new), `crates/ferric-tools/src/builtin/mod.rs`, `crates/ferric-tools/tests/builtin_file_tools.rs`
- **Depends on:** (none)
- **Success (EARS):**
  - WHEN run with `{query, path?, max_results?}`, **THEN** recurse from `ctx.workspace.resolve(path|".")`, read files as UTF-8 (skip read errors → binaries fall away), return `relpath:lineno:line` for lines containing the literal `query` — **sorted/deterministic** (ADR-008), **capped at `max_results`** (default 50, ADR-018), `relpath` via `strip_prefix(workspace.root())`.
  - WHEN walking, **THEN** skip noise dirs (`.git`, `target`, `node_modules`, `.ferric`).
  - **SHALL** declare `permission: Read`, `min_tier: Nano`, and override `target_paths` to return the search root (registry boundary-checks it; escapes refused, ADR-005).
- **Notes:** mirror `list_dir.rs`; substring (no `regex` dep); `Box::new(SearchFiles)` in `register_builtin_tools`.

### T-1202: Document the tool
- **Touches:** `README.md`, `docs/`
- **Depends on:** T-1201
- **Success (EARS):** docs **SHALL** list `search_files` (args + find-before-edit use) and append the Sprint 12 timeline entry.
