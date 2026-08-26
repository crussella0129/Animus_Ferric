Finalized - DO NOT EDIT

# Sprint 14 Build Plan — Formalize the tool rings

Make rings explicit (`ring` per tool), capability-gated (`ring_for_tier`), and
fix the alphabetical cap (trim from the outer ring first). `RunPolicy` unchanged
⇒ snapshot test untouched. Rationale: `sprints/s14/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** rings explicit, capability-gated, cap trims outside-in.
  - **A. The ring mechanism** — T-1401
  - **B. ADR + docs + re-bench** — T-1402

## Execution Sequence

### T-1401: `ring` field + ring-aware `tools_for_policy`
- **Touches:** `crates/ferric-tools/src/spec.rs`, `crates/ferric-tools/src/builtin/*.rs` (8), `crates/ferric-core/src/scale.rs`, `crates/ferric-tools/src/registry.rs`
- **Success (EARS):**
  - `ToolSpec` `min_tier: Tier` → **`ring: u8`** (0 = core); builtins `ring: 0` except `search_files` + `move_path` → `ring: 1`.
  - `ferric-core::ring_for_tier(Tier) -> u8` (Nano→0, Small→1, Medium→2, Large/Xl/Ultra→3).
  - `tools_for_policy`: keep `ring ≤ ring_for_tier(tier)`; over `max_tools` → **trim highest ring first** (select by `(ring, name)`, truncate); return **name-sorted** (ADR-008).
- **Notes:** compiler enumerates the `min_tier` sites; update the dummy `TestTool`; add a trim-from-outer test; existing cap test stays green (all ring 0).

### T-1402: ADR + docs + re-bench
- **Touches:** `decisions.md`, `README.md`, `docs/`
- **Success (EARS):** ADR records the ring architecture (ring field + `ring_for_tier` + trim-from-outer, superseding the alphabetical cap; `measured_level` promotes the ring set). README/docs describe Ring 0 vs Ring 1 + the gate. Sprint 14 timeline entry appended.

## Post-build (test)
- Unit tests (`ring_for_tier`, trim-from-outer, Nano→6 / Small→8) + the E2E re-bench (all 8 still solid at Small tier).
