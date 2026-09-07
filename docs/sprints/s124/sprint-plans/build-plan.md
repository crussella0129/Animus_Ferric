Finalized - DO NOT EDIT

# Sprint 124 Build Plan

## Intents
- [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) — state: active; acceptance criteria covered: AC-4 first increment (extract `server.rs`'s ~11.7K-line test module into a submodule; production-cluster splits remain active follow-on).

## Schema Tree
- Sprint Goal: server.rs is navigable (18.3K → 6.5K production)
  - Test-module extraction
    - T-12401: server.rs → server/mod.rs + server/tests.rs (atomic move)

## Execution Sequence

### T-12401: Extract server.rs's test module into a submodule
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) (AC-4, first increment)
- **Touches:** `crates/ferric-cli/src/server.rs` (deleted), `crates/ferric-cli/src/server/mod.rs` (new), `crates/ferric-cli/src/server/tests.rs` (new)
- **Depends on:** (none)
- **Acceptance criterion:** INT-0009 AC-4 — the largest file is broken along a real seam (production vs. its trailing test module) without changing shipped behavior; `crate::server::X` and `crate::server::tests::X` paths preserved.
- **Success criterion (EARS):**
  - **WHEN** the crate is built, **THEN** `server.rs` no longer exists, `server/mod.rs` and `server/tests.rs` do, and `cargo build -p ferric-cli` **SHALL** succeed.
  - **WHEN** `cargo test --workspace` runs, **THEN** it **SHALL** pass unchanged, including the `server::tests::*` module and the `server_resolution` cross-module calls into `crate::server::tests::`.
  - **WHEN** the production region (lines 1-6558) of `server/mod.rs` is diffed against the old `server.rs`, **THEN** the only production-side change **SHALL** be the inline `mod tests { … }` becoming `mod tests;` (no logic drift).
- **Notes:** mechanical `head`/`sed` split on line boundaries; `mod server;` in `lib.rs` auto-resolves to `server/mod.rs`. The 7 small inline `#[cfg(test)]` blocks in the production region stay in `mod.rs`; only the single large trailing `pub(crate) mod tests` moves, keeping its `use super::*;`. Behavior-preserving — the moved tests passing unchanged is the proof.
