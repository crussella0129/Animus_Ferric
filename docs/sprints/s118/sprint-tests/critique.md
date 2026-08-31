# Test Critique — Sprint 118

## Concerns

### C-001: Frozen local-path negative row remains unexecuted
- **Where:** `test-plan.md` / `tailscale_pre_mutation_failures_never_apply`; `unit-tests.md` / Accepted frozen-plan deviations; `integration-tests.md` / Regression boundary
- **Quote:** "capture/collision/readiness/identity/listener/local-path/publication fault table"
- **Failure mode:** negative-path
- **Why it matters:** The finalized plan named a deterministic local-path-absolutization failure row, but `std::path::absolute` has no injectable failure seam or portable invalid-path coordinate, so that row remains unexecuted. The evidence now states this accurately; the governing EARS outcomes for capture, collision, readiness, inspection, publication, mutation exclusion, and compensation are independently proved.
- **Suggested response:** defer-with-rationale — retain the explicit accepted-deviation record and do not represent this descriptive row as passed.

### C-002: Frozen package doc-test command remains red
- **Where:** `test-plan.md` Frozen Commands item 7; `unit-tests.md` / Frozen and regression commands
- **Quote:** "`cargo test -p ferric-cli --doc` | not applicable and exited 1"
- **Failure mode:** evidence-drift
- **Why it matters:** The immutable command cannot run because `ferric-cli` has only binary targets with doctests disabled. The supplemental workspace doc-test command passed the applicable library surface, and no Sprint 118 EARS clause depends on doctests, but the original frozen command itself is still not green.
- **Suggested response:** defer-with-rationale — preserve the exact red result, binary-target metadata explanation, and supplemental workspace result without converting the frozen command into a pass.

## Confidence
proceed-with-caveats
