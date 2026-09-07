# Test Critique — Sprint 124

## Concerns

### C-001: Behavior preservation rests on the moved suite + a mechanical diff — the strongest available proof for a move
- **Where:** `unit-tests.md` / `e2e-tests.md`
- **Quote:** "the production region (lines 1-6559) is byte-identical to the old `server.rs`"
- **Failure mode:** weak-assertion (screened, accepted)
- **Why it matters:** a file move could silently alter production code or drop a test.
- **Suggested response:** accept. Two independent checks make drift impossible to miss: (1) a byte-for-byte `git diff` of the production region proves no production line changed except the `mod tests { … }` → `mod tests;` declaration; (2) the ~11.7K relocated tests running green from the new file prove no test was dropped or broken. This is a stronger guarantee than any new hand-written test.

### C-002: The one path-resolution risk is explicitly exercised
- **Where:** `integration-tests.md`
- **Quote:** "`server_resolution.rs` calls `crate::server::tests::…` … still compile and pass"
- **Failure mode:** missing-risk (screened, resolved)
- **Why it matters:** the `pub(crate) mod tests` is referenced cross-module; a wrong module form would break `server_resolution`'s tests.
- **Suggested response:** none — covered and green.

## Confidence
clean

INT-0009 AC-4's first increment is verified by build, full-suite, feature-shape,
and byte-diff checks; the only non-obvious risk (the cross-module test path) is
green. No production behavior changed; the production-cluster splits remain
deferred AC-4 increments on the now-navigable file.
