# Sprint 14 Research Report — Formalize the tool rings

> Ring 0 is built + measured 100% to 1B (sprint 13). Now make the **rings** real:
> each tool declares a `ring`; the active rings are chosen by capability; and the
> `max_tools` cap **trims from the outer ring first** so the core is never dropped.
> This both delivers the user's north star and fixes a now-real bug.

## Decisions Reviewed
- **Rings north star** ([[ferric-tool-rings]]): ringed tool sets widen with proven reliability; active rings = the grammar. Ring 0 = the navigate/mutate core (6 tools); Ring 1 = `search_files`, `move_path`; Ring 2 = planner/diff; Ring 3 = MCP/external.
- **The latent bug, now real (registry.rs:104):** `tools_for_policy` filters by `min_tier ≤ tier` then `.take(max_tools)` **after an alphabetical sort**. With 8 builtins all at `min_tier: Nano` and the Nano cap of 6, a Nano run today drops the **2 alphabetically-last** tools (`search_files`, `write_file`) — losing `write_file`, an essential. The rings fix replaces the alphabetical cap with **trim-from-outer**.
- **ADR-006** — config-supplied, never inferred: the active ring derives from the model's `tier` (which comes from `params_b` or the **`measured_level`** override, ADR-019) — so promotion is config/measurement-driven, not sniffed.
- **ADR-008** — deterministic enumeration: tool output stays name-sorted.

## Existing code survey
| File | Change |
|------|--------|
| `crates/ferric-tools/src/spec.rs` | `ToolSpec.min_tier: Tier` → **`ring: u8`** (0 = core). |
| `crates/ferric-tools/src/builtin/*.rs` (8 tools) | each `min_tier: Tier::Nano` → `ring: 0` **except** `search_files` + `move_path` → `ring: 1`. |
| `crates/ferric-core/src/scale.rs` | add `pub fn ring_for_tier(Tier) -> u8` (Nano→0, Small→1, Medium→2, Large/Xl/Ultra→3) — the capability→ring map, beside `tier_for_*`. |
| `crates/ferric-tools/src/registry.rs` | `tools_for_policy`: filter `ring ≤ ring_for_tier(tier)`, then **trim from the outer ring first** (cap by ring-priority), present name-sorted; the dummy `TestTool` gains a `ring`; add a trim-from-outer test. |
| `crates/ferric-core/tests/tier_table_snapshot.rs` | **untouched** — `RunPolicy` is unchanged (the ring is derived from `tier`, not stored), so `max_tools`/tier rows don't move. |

## Design (settled)
- **`ring_for_tier`:** Nano→0, Small→1, Medium→2, Large→3, Xl→3, Ultra→3 (rings 2–3 are reserved for future planner/MCP tools; today only rings 0–1 exist).
- **Ring assignment:** Ring 0 = `read_file, write_file, edit_file, list_dir, make_dir, delete_path` (the user's open/edit/create/delete core, measured `solid` to 1B). Ring 1 = `search_files, move_path` (find & organize). *(The s13 100% was at the Small-tier bench profile, so it exercised all 8; gating Nano to the 6-core is the conservative "smallest grammar for the smallest model" default the rings model is for — the user's flagged Ring-0/1 fork resolved toward their own sketch.)*
- **Trim-from-outer `tools_for_policy`:** filter `ring ≤ ring_for_tier(tier)`; pick the kept set by `(ring asc, name)` priority and `truncate(max_tools)` — so the cap sheds the **highest** rings, never the core; then re-sort by `name` for a deterministic, name-sorted result (ADR-008). The grammar is built from this set → "active rings = the grammar", now literally curated.
- **Control / promotion:** the active ring follows `tier`, and `measured_level` overrides `tier` bidirectionally (ADR-019) — so a model is promoted/pinned to a ring set by its measured reliability or explicit config. (A dedicated `--max-ring` CLI override is a clean follow-on; the `measured_level` path already gives "control exactly what rings".)

## Risks / unknowns
- **Replacing `min_tier` is a 10-file edit** — mechanical (struct field rename + per-tool value); the compiler enumerates every site. No behaviour change for existing tiers except the intended Nano curation.
- **Nano now gets 6 tools, not 8** — by design (the core). `search_files`/`move_path` unlock at Small+. If the user wants them in Ring 0, it's a one-line ring change per tool.

## Recommended approach
T-1401: the `ring` field + `ring_for_tier` + ring-aware trim-from-outer `tools_for_policy` + the 8 ring assignments + a trim test. T-1402: ADR (the ring architecture, superseding the alphabetical cap) + README/docs + a re-toolbench confirming Ring 0 stays 100% and Ring 1 is exercised at Small tier.
