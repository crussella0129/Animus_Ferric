Finalized - DO NOT EDIT

# Sprint 11 Build Plan — Re-enable constrained decoding on mistral.rs

`MistralRsProvider::complete()` strips the constraint (ADR-020 workaround). The
hang is fixed in 0.8.15 (ADR-025) and mistralrs exposes
`RequestBuilder::set_constraint(Constraint{JsonSchema/Lark/Regex})`. Wire it,
then the probe decides enforcement. Rationale:
`sprints/s11/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** pass our `Constraint` to the mistralrs engine; the probe decides enforcement.
  - **A. Wire the constraint** — T-1101

## Execution Sequence

### T-1101: Pass `Constraint` to the mistralrs `RequestBuilder`
- **Touches:** `crates/ferric-provider/src/mistralrs.rs`
- **Depends on:** (none)
- **Success (EARS):**
  - WHEN `complete()` gets `Some(Constraint::…)`, **THEN** map via a pure `to_mistralrs_constraint(&Constraint) -> mistralrs::Constraint` (`JsonSchema→JsonSchema`, `Lark→Lark`, `Regex→Regex`) and apply `builder.set_constraint(…)`; WHEN no constraint, behaviour **SHALL** be unchanged.
  - `capabilities().supports_constraint` **SHALL** stay **`false` provisionally** — wiring present, capability unadvertised until the probe proves enforcement-without-hang.
- **Notes:** replace the strip comment (`mistralrs.rs:139`). Mapping fn + test are `#[cfg(feature="backend-mistralrs")]`; test via `matches!` (type may lack `PartialEq`). ADR-010 holds (mistralrs strips tools).

## Post-build (test → loop)
- **Test:** re-run `grammar_probe` (`trivial`, then `unified`) through the wired provider on the 1B GGUF, bounded subprocess → enforce / ignore / hang.
- **Loop (ADR-027):** enforce → flip `supports_constraint:true` + README; ignore/hang → guard `set_constraint` off + document the definitive verdict.
