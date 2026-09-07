# Sprint 123 Integration Tests

Tested head: `d038ec6`.

## Fixture binary identity, end to end (the load-bearing regression proof)
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) (AC-1, fixture identity preserved)
- `crates/ferric-cli/tests/server_lifecycle_fixture.rs` (existing, unchanged), run with
  `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture`:
  **5 passed / 0 failed.** These spawn the real `ferric-lifecycle-test` binary
  (now a thin shim over the library), and the fixture LocalAPI transport still
  activates — proving the `CARGO_BIN_NAME` → set-once `bin_identity` threading
  preserved the exact gate behavior across the extraction. This is the highest-risk
  path and it is green untouched.
