# Sprint 123 Unit Tests

Tested head: `d038ec6`. This is a behavior-preserving refactor, so the primary
evidence is the whole existing suite passing unchanged; the one new unit test
covers the seam the change introduces.

## T-12301 unit tests
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md)
- `name_is_lifecycle_fixture_matches_only_the_fixture_binary` (`crates/ferric-cli/src/bin_identity.rs`): `"ferric-lifecycle-test"` → true; `"ferric"`, `""`, `"ferric-lifecycle-fixture"` → false. **PASS** (EARS: the identity predicate matches only the fixture binary). This is the testable core of the `env!("CARGO_BIN_NAME")` → threaded-identity change.

## Whole-suite behavior preservation (the load-bearing evidence for a refactor)
- `cargo test --workspace --locked` — all suites pass; `ferric-cli`'s library test binary is **404 passed / 0 failed** (403 relocated from the old bin target + the new `bin_identity` test). The two `routing_tests` and every other module test moved from the bin crate to the library crate and pass unchanged.
- `crates/ferric-cli/tests/source_execution.rs::source_quality_and_feature_matrix` — updated to read the feature-gated `mod startup;` declaration from `lib.rs` (where the command surface now lives) instead of `main.rs`; **PASS** (2/2 in that suite).
