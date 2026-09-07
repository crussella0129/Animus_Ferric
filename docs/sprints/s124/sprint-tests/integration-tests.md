# Sprint 124 Integration Tests

Tested head: `e77f749`.

## Cross-module test-helper path preserved (the one non-obvious risk)
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) (AC-4)
- `crates/ferric-cli/src/server_resolution.rs` calls
  `crate::server::tests::legacy_tailscale_registration_remains_unowned()` and
  `crate::server::tests::promised_origin_static_matrix_precedes_process_inspection()`
  from its own tests. After the move, these still compile and pass — proving the
  `#[cfg(test)] pub(crate) mod tests;` submodule kept the `crate::server::tests::`
  path resolving. This was the only thing the move could have broken, and it is green.

## Lifecycle fixture (the server crate's heaviest suite)
- `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture` — **5/5**, unchanged. The server lifecycle behavior is unaffected by relocating the module's tests.
