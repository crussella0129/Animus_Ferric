Finalized - DO NOT EDIT

# Sprint 15 Build Plan — `--max-ring` ring override

An explicit, restrict-only cap on the active rings (the user's "control exactly
what rings"). `RunPolicy.max_ring` + a `min(tier_ceiling, override)` in
`tools_for_policy` + a CLI flag. Rationale: `sprints/s15/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** an explicit operator cap on the active rings.
  - **A. The mechanism** — T-1501
  - **B. CLI + docs** — T-1502

## Execution Sequence

### T-1501: `RunPolicy.max_ring` + ring cap in `tools_for_policy`
- **Touches:** `crates/ferric-core/src/scale.rs`, `crates/ferric-tools/src/registry.rs`, `crates/ferric-loop/src/protocol.rs`, `crates/ferric-loop/tests/common/mod.rs`
- **Success (EARS):**
  - `RunPolicy.max_ring: Option<u8>` (None = `ring_for_tier(tier)`); `policy_for` sets None.
  - `tools_for_policy` ceiling = `ring_for_tier(tier).min(max_ring.unwrap_or(u8::MAX))` (cap-only; trim-from-outer unchanged; no signature change).
  - the two `RunPolicy{}` test helpers add `max_ring: None`.
- **Notes:** snapshot test untouched (field-assert style). Unit test: `Some(0)` on Small → only ring-0; `None`/`Some(1)` → all 8; `Some(5)` no-op.

### T-1502: `--max-ring` CLI flag + docs
- **Touches:** `crates/ferric-cli/src/{query.rs,toolbench_cmd.rs}`, `crates/ferric-cli/tests/cli.rs`, `README.md`, `decisions.md`
- **Success (EARS):**
  - `ferric query --max-ring <u8>` sets `policy.max_ring` (after `policy_for`); `ferric toolbench --max-ring` benches rings `0..=N`.
  - docs: override is restrict-only (expand via `measured_level`), `--max-ring 0` = core-only; ADR-028 amendment; Sprint 15 timeline entry.
- **Notes:** `--mock` CLI test via the trace `PromptAssembled.offered_tools` (=6 core at `--max-ring 0`).
