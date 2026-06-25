# Sprint 18 Research Report — Round out Ring 1 (`find_files` + `copy_file`)

> The rings *system* is complete (defined → controllable → measured → durable,
> s14–17). Now *widen* it. Ring 1 is the "find & organize" ring but has only
> `search_files` (find by content) + `move_path`. The obvious gaps: **find by
> *name*** and **copy** (the organize complement to move). Two small, safe
> (pure-filesystem, ADR-005-clean) builtins that round Ring 1 into a coherent
> set — and give calibration a fuller ring to sweep.

## Decisions Reviewed
- **ADR-028** — rings: `ToolSpec.ring`, `ring_for_tier` ceiling, trim-from-outer. Both new tools are `ring: 1` (admitted at Small+, absent at Nano). This is a Ring-1 round-out, within the architecture — an ADR-028 amendment, not a new ADR.
- **ADR-008** — deterministic, sorted output. `find_files` walks sorted (like `search_files`) and returns name-sorted relpaths.
- **ADR-018** — result caps. `find_files` caps results (default 100).
- **ADR-005** — no external execution. Both tools are workspace-scoped `std::fs` only; no new surface.

## Capacity check (grounded in `scale.rs`)
`tier_row` max_tools: **Nano = 6, Small = 10**, Medium = 16, … Ring 0 (6) + Ring 1
(2 today → **4** after this sprint) = **10**, which **exactly fits Small's cap** —
Small gets all 10, Nano still gets exactly the 6 core (Ring 1 absent). No trimming
at Small; the `rings_gate_builtins_by_tier` test moves from 8 → 10.

## Existing patterns to mirror
| New tool | Template | Delta |
|---|---|---|
| `find_files` (Read, ring 1) | `builtin/search_files.rs` | match the **filename** (substring) instead of line contents; emit `relpath` (not `relpath:lineno:line`); same sorted walk, noise-dir skip, `max_results` cap. |
| `copy_file` (Write, ring 1) | `builtin/move_path.rs` | `std::fs::copy` instead of `rename`; both endpoints declared in `target_paths` (boundary + denylist guarded); file-only (error on a directory source). |

`mod.rs` registers both; `tests/builtin_file_tools.rs` adds per-tool tests + bumps
`rings_gate_builtins_by_tier` to 10 (asserting the new names appear at Small, absent
at Nano).

## Design (settled)
- **`find_files`** — `{pattern: string, path?: ".", max_results?: 100}`; recurse from `path`, push each file whose **name contains `pattern`** as a workspace-relative path, sorted, capped, skipping `.git/target/node_modules/.ferric`. Read permission. The name-search companion to `search_files`' content search.
- **`copy_file`** — `{from: string, to: string}`; resolve+guard both, `create_dir_all` the destination parent, `std::fs::copy`. Errors if `from` is a directory (file copy only — recursive dir copy is out of scope). Write permission, so the destination denylist (`.ferric`, `.git/config`, ssh keys) applies.

## Risks
- **`find_files` vs `search_files` confusion** — distinct + documented (name vs content); both are genuinely needed for navigation. Description makes the distinction explicit.
- **`copy_file` directory source** — explicitly errored (not silently partial), mirroring how `delete_path` gates non-empty dirs.

## Recommended approach
T-1801: `find_files` (Ring 1) + tests. T-1802: `copy_file` (Ring 1) + tests + bump
the rings-gate test to 10. T-1803: docs (README builtin list + Sprint 18 timeline +
ADR-028 amendment) + re-bench (`--calibrate-rings` still `solid` through Ring 1 with
the fuller set). AI-verifiable via the builtin unit tests + the rings-gate count; the
ollama re-bench confirms the wider Ring 1 still fires 100%.
