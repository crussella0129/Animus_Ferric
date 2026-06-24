# Sprint 13 Research Report — Complete Ring 0 (`edit_file` + `delete_path`) + prove 100%

> Per the user's **concentric-rings** direction (north star): Ring 0 is the
> always-on core of navigate/mutate verbs. It's missing two — surgical **edit**
> and **delete** — and the user's standing requirement is **retained 100% toolcall
> reliability**. This sprint builds both on the proven tool pattern and re-runs the
> toolbench to *measure* the full core's fire rate.

## Decisions Reviewed
- **Rings north star** (project memory `ferric-tool-rings`): Ring 0 = `read_file, list_dir, write_file, make_dir, edit_file(NEW), delete_path(NEW)` (+ `task_complete`). The grammar = active rings. The `min_tier`/alphabetical-cap fix is **sprint 14** (the ring formalization); this sprint only fills the two Ring-0 gaps.
- **ADR-005** — security hardcoded: both tools resolve through `Workspace` and declare `target_paths`; `delete_path` declares `Write`, so `check_write_target` applies the **denylist** (`.ferric`, `.git/config`, ssh keys → auto-Denied) exactly like `write_file`/`move_path`. **Zero new security surface.**
- **ADR-008 / ADR-018** — deterministic, bounded output (trivially satisfied — both tools return a short status line).
- **ADR-019** — the toolbench fire-rate is the reliability instrument; "retain 100%" becomes a measured number, not a hope.

## Existing code survey (the pattern to reuse)
| File | Relevance |
|------|-----------|
| `crates/ferric-tools/src/builtin/write_file.rs` | the Write-permission + `resolve` template `edit_file` mirrors (read → modify → write). |
| `crates/ferric-tools/src/builtin/move_path.rs` | the closest destructive analog for `delete_path` (Write, boundary, missing-source error). |
| `crates/ferric-guard/src/checker.rs` | `PermissionLevel{Read,Write,Execute}`; Write/Execute → `check_write_target` (denylist). |
| `crates/ferric-tools/src/builtin/mod.rs` | register both; `path_arg` helper. |
| `crates/ferric-cli/src/toolbench_cmd.rs` | the reliability run uses `ModelProfile{params_b:8.0}` → **Small tier, max_tools=10**, so the full 8-tool core is benched (no cap truncation this sprint). |

## Design (settled)
- **`edit_file`** — `{path, old_string, new_string}`. Resolve (`Write`), read the file, replace the **first** occurrence of `old_string` with `new_string`, write back. Error if the file is unreadable, `old_string` is empty, or `old_string` is absent (the model retries with better context). First-occurrence (not require-unique) maximizes fire rate for small models; edit *correctness* is the model's job, fire *rate* is what we gate. `permission: Write`, `min_tier: Nano`.
- **`delete_path`** — `{path, recursive?: bool}`. Resolve (`Write`, denylist-checked), then: a file → remove; an **empty** dir → remove; a **non-empty** dir → remove only if `recursive: true`, else a clear error. Safety default: a small model can't accidentally nuke a tree. Error if the path is missing. `permission: Write`, `min_tier: Nano`.

## Risks / unknowns
- **Destructive `delete_path`** — mitigated by: the guard boundary + denylist (no escaping, no `.ferric`/git/ssh), the non-empty-dir `recursive` gate, and the registry trace recording every call. (User flagged a possible extra confirmation; deferred — the recursive gate + denylist is the safety floor; an interactive confirm doesn't fit the headless tool contract and can be a Ring/policy concern later.)
- **The alphabetical `max_tools` cap** now actually bites at **Nano** tier (8 tools > cap 6) — but the **toolbench runs at Small tier**, so it benches all 8. The Nano-cap fix is **sprint 14's** ring-aware trim. Noted, not blocking.

## Recommended approach
Build `edit_file` + `delete_path` on the `write_file`/`move_path` pattern (guard-scoped, deterministic, tested), register them, then **re-run `ferric toolbench --backend openai --protocol grammar` against ollama** (qwen2.5-coder:7b and llama3.2:1b) and report the per-tool fire rate for the now-complete Ring 0 — the measured "100% reliability" gate. Ring formalization (the `ring` field + trim-from-outer + promotion) is sprint 14.
