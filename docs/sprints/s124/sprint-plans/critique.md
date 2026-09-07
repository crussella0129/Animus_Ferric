# Plan Critique — Sprint 124

## Concerns

### C-001: No new test is planned — correct for a behavior-preserving move, but state the assertion
- **Where:** `test-plan.md` Unit Tests
- **Quote:** "No new unit test. This is a behavior-preserving file move; the ~11.7K lines of `server::tests::*` that move are the assertion"
- **Failure mode:** plan-test-mismatch (screened, accepted)
- **Why it matters:** every EARS clause needs verification; a reader could mistake "no new test" for "untested."
- **Suggested response:** accept. The clauses are verified by named checks — `cargo build -p ferric-cli` (structure), `cargo test --workspace` (the moved `server::tests::*` and the `server_resolution` cross-module helpers pass unchanged), and a production-region `git diff` (no drift). Writing a new libtest to assert a file moved would test the move, not a behavior. The ~11.7K relocated tests running green from `server/tests.rs` is a stronger assertion than any hand-written one.

### C-002: The only non-obvious risk (cross-module `crate::server::tests::`) has explicit coverage
- **Where:** `test-plan.md` Integration Tests / research report
- **Quote:** "`server_resolution.rs` calls `crate::server::tests::…` … proves the `pub(crate) mod tests;` submodule kept the path resolving"
- **Failure mode:** missing-risk (screened, resolved)
- **Why it matters:** if the move broke the `pub(crate)` test-module path, `server_resolution`'s tests would fail to compile.
- **Suggested response:** none — covered; the workspace suite builds and runs both.

## Confidence
clean

The single EARS-bearing task maps to build, suite, and structural-diff
verifications; the one real risk (the cross-module test-helper path) is
explicitly exercised. No intent boundary is crossed — this moves only the test
module; the production-cluster splits remain deferred AC-4 increments.
