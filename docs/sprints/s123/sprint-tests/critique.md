# Test Critique — Sprint 123

## Concerns

### C-001: The compile-time → runtime identity shift is now proven, not just argued
- **Where:** `integration-tests.md` / `bin_identity.rs` + `tailscale_localapi.rs`
- **Quote:** "the fixture LocalAPI transport still activates — proving the `CARGO_BIN_NAME` → set-once `bin_identity` threading preserved the exact gate behavior"
- **Failure mode:** weak-assertion (screened, resolved)
- **Why it matters:** the plan's C-001 flagged that the identity check moved from a compile-time `env!` constant to a value set at `run` startup. If the threading were wrong, the fixture transport would silently not activate.
- **Suggested response:** none. `server_lifecycle_fixture` (5/5) spawns the real `ferric-lifecycle-test` binary and exercises that transport end to end, and the pure `name_is_lifecycle_fixture` test pins the predicate. The concern is closed by evidence.

### C-002: Behavior preservation rests on existing coverage — appropriate for a move
- **Where:** whole-suite evidence
- **Quote:** "the whole existing suite passing unchanged … is the E2E"
- **Failure mode:** intent-coverage (screened, accepted)
- **Why it matters:** a large code move is only as safe as the suite that guards it; behavior exercised by no test could shift unnoticed.
- **Suggested response:** accept. The code moved within one crate (all `crate::` paths intact); every test moved with it and `cargo test --workspace` runs the lib, bin, and integration targets together, so no test was dropped. The relocated source-structure guard (`source_execution.rs`) was updated to the new file and re-verified. This is the standard, bounded guarantee for a behavior-preserving refactor.

## Confidence
clean

INT-0009 AC-1's every EARS clause maps to a named, executed test with a tight
assertion, including the highest-risk path (the lifecycle fixture) and the exact
warning the human trial surfaced. No intent boundary is crossed; AC-2/AC-4 and
the server/serving splits remain explicitly deferred.
