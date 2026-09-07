# Sprint 124 Unit Tests

Tested head: `e77f749`. This is a behavior-preserving file move, so the ~11,700
relocated `server::tests::*` lines are the assertion — no new unit test is
written (one would test the move, not a behavior).

## T-12401
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md)
- The entire `server::tests::*` module (moved into `crates/ferric-cli/src/server/tests.rs`) compiles and runs from its new location: `cargo test --workspace` is green with **0 failures**, and the module's tests appear under `server::tests::` exactly as before.
- **No production drift (verified mechanically):** `git show HEAD~2:…/server.rs | head -6559` diffed against `server/mod.rs` lines 1-6559 is **byte-identical**; the only production-side change is the trailing inline `mod tests { … }` becoming `mod tests;`.
