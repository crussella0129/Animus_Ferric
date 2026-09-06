# Test Critique — Sprint 121

## Concerns

### C-003: Historical Windows failure remains unexplained

- **Where:** `sprint-tests/ci-checkpoint-001.md`, `ci-checkpoint-002.md`, `ci-checkpoint-003.md`, `windows-reset-correction.md`; INT-0008 qualification boundary.
- **Quote:** "The historical uninstrumented failure's cause remains unknown."
- **Failure mode:** flake-risk
- **Why it matters:** The correction demonstrably fixes a connection-reset fixture defect, but neither its injected regression nor subsequent successful qualification establishes the original failure’s cause. Corrected-source acceptance must not become a claim of a diagnosed production ownership repair or arbitrary parallel-run robustness.
- **Suggested response:** defer-with-rationale — retain the original failure, diagnostic non-reproduction and mutation control alongside corrected-source evidence. Preserve the existing native-admission/parallel-robustness backlog and fail closed on future canonical recurrence. No additional Test-stage change is required for this documented caveat.

C-001 is resolved for corrected source `a417c5d00361fd25a238346e5015fb07ed5ae7c7`: fresh Windows formatting, included-fixture formatting, workspace Clippy and canonical tests passed; the retained 79 suite confirmations total 1,303 passes/13 documented ignores. Exact-head CI `34004554100` passed all eight required jobs. Fresh `live-test-002` passed with the unchanged model/runtime, explicit 1024 cap, original deadlines and checked cleanup. I independently verified its report/source hashes and recomputed its retained trace digest.

C-002 is resolved: the actual fixture accept loop narrowly preserves its listener across connection resets, retains fatal-error refusal and the absolute deadline, and has composed reset/fatal regressions plus deterministic deadline coverage. The executed mutation control fails under the old behavior after checked cleanup.

The updated Python-grader confirmations now match the evidence text. The locked plans, linked intent chapters, completed entries and current unit/integration/E2E records provide adequate named coverage for the Test-stage promises. Actual wire observations, exact-byte assertions, no-clobber evidence matrices and diagnostic calibration guards are substantive positive controls.

E04-C’s later Loop reconciliation, separate fresh post-Loop audit, single PR and final PR-head checks remain mandatory—not certified by this Test review. Neither intent is realized; larger-model application, hardware calibration, reasoning/compaction and broader platform guarantees remain deferred.

## Confidence

proceed-with-caveats
