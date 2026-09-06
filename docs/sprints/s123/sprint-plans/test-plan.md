# Sprint 123 Test Plan

## Intent Traceability
| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|--------|----------------------|--------------------------|--------------|
| [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) | AC-1 — duplicate-source warning gone (T-12028) | T-12301 / WHEN build THEN no multi-target warning | `no_duplicate_source_build_warning` (build-property check) |
| [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) | AC-1 — fixture binary identity preserved | T-12301 / WHEN name predicate THEN matches only the fixture | `name_is_lifecycle_fixture_matches_only_the_fixture_binary` (unit) |
| [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) | AC-1 — fixture transport activates | T-12301 / WHEN fixture spawns the binary THEN transport activates | existing `tests/server_lifecycle_fixture.rs` pass unchanged |
| [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) | AC-3 — behavior-preserving | T-12301 / WHEN suite runs THEN passes unchanged | full workspace `cargo test` + `lifecycle-fixture` + `no-default-features` shapes |

## Unit Tests
### T-12301 unit tests
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md)
- `name_is_lifecycle_fixture_matches_only_the_fixture_binary` (`crates/ferric-cli/src/bin_identity.rs`): `"ferric-lifecycle-test"` → true; `"ferric"`, `""`, `"ferric-lifecycle-fixture"` → false. This is the testable core of the `CARGO_BIN_NAME` → threaded-identity change.
- Stubs: none (pure predicate). The `OnceLock` wrapper is exercised by the integration path, not a global-mutating unit test.

## Integration Tests
### Fixture binary identity end to end
- **Intents:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md)
- `tests/server_lifecycle_fixture.rs` (existing, unchanged) spawns `CARGO_BIN_EXE_ferric-lifecycle-test` and asserts the fixture LocalAPI lifecycle. Passing unchanged after the extraction proves the identity threading preserved the exact gate behavior — the load-bearing regression proof.

## End-to-End Tests
- **Status:** possible
- `no_duplicate_source_build_warning`: run `cargo build -p ferric-cli` and assert the output contains no "found to be present in multiple build targets" line (the exact warning from the human trial). Verified in the Test phase against the real build; recorded with the tested head.
- The whole-workspace suite passing unchanged (incl. the `lifecycle-fixture` and `no-default-features` feature shapes and the aarch64 `cargo check`) is the behavior-preservation E2E for a refactor of this kind.
