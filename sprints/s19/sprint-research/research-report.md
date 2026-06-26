# Sprint 19 Research Report — Seed Ring 2 (`multi_edit`) + bench higher rings

> Ring 0 (navigate/mutate) and Ring 1 (find & organize) are complete. **Ring 2 =
> "plan & apply structured changes."** Seed it with `multi_edit` — an ordered,
> **atomic** batch of edits to one file. It's the right Ring-2 tool *for small
> local models*: more capable than the Ring-0 `edit_file` (one change), but still
> reliably emittable (a JSON array of `{old,new}` strings) — unlike a
> line-numbered unified diff, which a small model can't construct. Plus a
> `--params-b` toolbench flag so calibration can actually *reach* Ring 2.

## Decisions Reviewed
- **ADR-028** — rings: `ToolSpec.ring`, `ring_for_tier` ceiling (Medium → 2), trim-from-outer. `multi_edit` is `ring: 2`; an ADR-028 amendment, not a new ADR.
- **ADR-008** — deterministic output (edits applied in array order; one write).
- **ADR-018** — output discipline (a concise applied-count summary).
- **ADR-005** — no external exec. `multi_edit` is pure `std::fs`; no new surface.

## Grounding (read the code)
- **`tier_for_params`** (`scale.rs:111`): 13–30 B → **Medium** → `ring_for_tier(Medium) = 2`. So `--params-b 20` lifts the toolbench to the Ring-2 ceiling.
- **Default `target_paths`** (`spec.rs:38`) extracts `path` → `multi_edit {path, edits}` is boundary + denylist guarded automatically (Write).
- **`edit_file.rs`** is the template: read-once, validate, `replacen(_, _, 1)`, write-once. `multi_edit` = that, looped over an `edits` array, atomically.
- **Toolbench profile** (`toolbench_cmd.rs:469`) hardcodes `params_b: 8.0` (Small → ring ceiling 1) — so today's sweep *cannot* reach Ring 2. A `--params-b` flag fixes that.

## Design (settled)
- **`multi_edit`** (`ring: 2`, Write) — `{path, edits: [{old_string, new_string}, …]}`. Read the file once; apply each edit **sequentially** to a working string via `replacen(old, new, 1)` (a later edit can touch text an earlier one inserted). **Atomic:** if any `old_string` is absent at its turn (or empty, or `edits` is empty) → error with **nothing written**; otherwise one `std::fs::write`. Returns `applied N edits to <path>`.
- **`toolbench --params-b <f32>`** (default 8.0) — replaces the hardcoded `8.0`, so `--params-b 20` benches at Medium (ring ceiling 2) and `--calibrate-rings` sweeps rings 0,1,2.

## Risk: can the local fleet even reach Ring 2?
The Nano/Small fleet (1B–8B) tops out at ring 1 by *tier*. With `--params-b 20`
the toolbench benches at the **Medium ceiling**, so the calibration sweep includes
Ring 2 and measures whether the model can actually drive `multi_edit` — regardless
of its nominal tier. That's the honest live test (a 7B asked to emit `multi_edit`).
If it fires `solid`, Ring 2 is reachable; if not, the calibration correctly caps it
at Ring 1 — exactly the demonstrated-reliability gate working. Either outcome is a
valid, informative result. (No 13B+ model needed; one is welcome later.)

## Recommended approach
T-1901: `multi_edit` builtin (Ring 2) + unit tests (atomic batch, missing-old
aborts with nothing written, empty edits/old errors) + `rings_gate_builtins_by_tier`
Medium case (11 tools incl. `multi_edit`; Small still 10). T-1902: `toolbench
--params-b` + docs (README builtin list / Ring 2; ADR-028 amendment; Sprint 19
timeline) + the live `--params-b 20 --calibrate-rings` sweep. AI-verifiable via the
builtin units + the gate count; the sweep reports whether the model drives Ring 2.
