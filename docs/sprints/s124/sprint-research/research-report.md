# Sprint 124 — Research Report

## Sprint Goal

Begin decomposing `server.rs` (INT-0009 **AC-4**, the flagship) with its safest,
highest-leverage first cut: extract the ~11,700-line `#[cfg(test)] mod tests`
into a submodule, converting `server.rs` → `server/mod.rs` + `server/tests.rs`.
This halves the file (18,257 → ~6,560 production lines) so the 74 production
types become legible, and is the prerequisite for cleanly splitting those types
into modules in later increments. Behavior-preserving — the moved tests, run
unchanged, are their own proof.

**Explicitly deferred (named so they don't evaporate):** splitting the 74
production types along their responsibility clusters — `cli` (Engine/ServerCommand/
args/config/runfile), `runtime` (spawn/health/clock traits), `managed`
(discovery/registration state), `doctor`, `publication` (Tailscale serve),
`launch`, `adoption` — are the immediate follow-up increments of AC-4, far
cleaner on a navigable 6.5K file than under 11.7K lines of tests. Doing the test
cut first is the sound order, not timidity.

## Existing Code Survey

- `crates/ferric-cli/src/server.rs` is **18,257 lines**. Production is ~6,558
  lines (74 top-level `struct`/`enum`/`trait` + 7 small inline `#[cfg(test)]`
  blocks); the remainder is **one** `#[cfg(test)]\npub(crate) mod tests {` block
  at lines 6560-18257 (~11,700 lines).
- **Public API to preserve.** Sibling files import `crate::server::{Engine,
  ManagedDiscoveryScope, ManagedServer, ManagedServerState, ManagedServerDiscovery,
  DiscoveryFingerprint, RegisteredServerSnapshot, ServerRunfile}` plus the
  functions `begin_managed_server_discovery_in`, `discover_managed_server_in`,
  `inspect_registered_server`, `with_registered_server_effect`. All of these are
  production items that stay in `server/mod.rs`, so every `crate::server::X`
  path is unaffected by moving only the tests.
- **The test module is `pub(crate)` for a reason.** `server_resolution.rs:865-866`
  calls `crate::server::tests::legacy_tailscale_registration_remains_unowned()`
  and `crate::server::tests::promised_origin_static_matrix_precedes_process_inspection()`
  from its own tests. Moving the block to `server/tests.rs` behind
  `#[cfg(test)] pub(crate) mod tests;` keeps `crate::server::tests::X` resolving.
- The 7 small inline `#[cfg(test)]` blocks in the production region are colocated
  with the code they test and **stay in `mod.rs`**; only the one large trailing
  module moves.

## External Sources

- Rust module resolution: `mod server;` resolves to either `server.rs` **or**
  `server/mod.rs`, never both — so this is a file move (delete `server.rs`,
  create `server/mod.rs`), not an ambiguity. A child `tests` submodule reaches
  its parent's private items through `use super::*`, exactly as the inline block
  does today, so no visibility change is needed for the tests.

## Risks / Unknowns / Dependencies

- **Verbatim move.** The 11.7K lines must move byte-for-byte; the block's leading
  `use super::*;` (and any other imports) go with it. A mechanical `head`/`sed`
  split plus a full-suite run is the check.
- **Cross-module test helpers.** `server_resolution`'s two calls into
  `crate::server::tests::` must still compile and pass — covered by running the
  workspace test suite, which builds both.
- **No production change.** Production lines 1-6558 are copied unchanged into
  `mod.rs`; the only production-side edit is replacing the inline `mod tests { … }`
  with `mod tests;`. Any accidental production drift would fail clippy/tests.
- **Scope discipline (INT-0009 AC-4).** This increment moves *only* the test
  module. It does not split any production cluster — those are the named
  follow-ups, kept out so this stays a low-risk, reviewable move.

## Recommended Approach

1. Create `crates/ferric-cli/src/server/`.
2. `server/mod.rs` = the production region (lines 1-6558) with the trailing
   inline `#[cfg(test)] pub(crate) mod tests { … }` replaced by
   `#[cfg(test)]\npub(crate) mod tests;`.
3. `server/tests.rs` = the block body (the ~11.7K lines between the braces),
   unchanged, keeping its `use super::*;`.
4. Delete `server.rs` (its `mod server;` in `lib.rs` now resolves to
   `server/mod.rs`).
5. Verify: `cargo fmt`/`clippy -D warnings`/`cargo test --workspace` all green
   (incl. the `server_resolution` cross-module test-helper calls), and the
   `lifecycle-fixture` + `no-default-features` shapes compile.

## Intents Reviewed

- [INT-0009 — Lean, decomposed harness architecture](../../intents/INT-0009-lean-decomposed-architecture.md)
  — **selected** (active). This sprint is the first increment of **AC-4**
  (large-file module splits, `server.rs` the flagship); the production-cluster
  splits remain active follow-on work under the same AC.

## Referenced Artifacts

- This report: `docs/sprints/s124/sprint-research/research-report.md`
- Direction plan (context): `docs/plans/2026-09-06-direction-and-refactor.md`
