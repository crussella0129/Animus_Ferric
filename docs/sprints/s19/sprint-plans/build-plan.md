Finalized - DO NOT EDIT

# Sprint 19 Build Plan — Seed Ring 2 (`multi_edit`) + bench higher rings

Seed Ring 2 ("plan & apply structured changes") with `multi_edit` — an ordered,
atomic batch of edits to one file (the right Ring-2 tool for small models: more
than `edit_file`, still reliably emittable vs a unified diff). Add `toolbench
--params-b` so calibration can reach Ring 2 (`--params-b 20` → Medium → ceiling 2).
Rationale: `sprints/s19/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** Ring 2 seeded + reachable by calibration.
  - **A. multi_edit** — T-1901
  - **B. --params-b + docs + live bench** — T-1902

## Execution Sequence

### T-1901: `multi_edit` (Ring 2, Write)
- **New:** `crates/ferric-tools/src/builtin/multi_edit.rs` + register in `mod.rs`.
- **Mirror:** `edit_file.rs` (read-once / validate / `replacen(_,_,1)` / write-once); default `target_paths` guards `path`.
- **Success (EARS):** `multi_edit {path, edits:[{old_string,new_string},…]}` applies edits **sequentially** to a working string, writes **once**; **atomic** — empty `edits`, empty `old_string`, or an absent `old_string` at its turn → error, nothing written; `ring: 2`, Write; returns `applied N edits to <path>`.
- **Tests:** 2-edit batch (incl. editing earlier-inserted text); missing old → file unchanged; empty edits/old → error. Bump `rings_gate_builtins_by_tier`: Medium (params 20) → 11 incl. `multi_edit`; Small still 10.

### T-1902: `toolbench --params-b` + docs + live Ring-2 bench
- **Touches:** `crates/ferric-cli/src/toolbench_cmd.rs`, `README.md`, `decisions.md`, `docs/testbench.md`.
- **Success (EARS):** `toolbench --params-b <f32>` (default 8.0) replaces the hardcoded `8.0`; `--calibrate-rings --params-b 20` sweeps rings 0,1,2. README Ring-2 mention; ADR-028 sprint-19 amendment; Sprint 19 timeline.

## Post-build (test)
- multi_edit units + rings-gate (Medium=11/Small=10) + the live `--params-b 20 --calibrate-rings` sweep (does a 7B drive Ring 2?).
