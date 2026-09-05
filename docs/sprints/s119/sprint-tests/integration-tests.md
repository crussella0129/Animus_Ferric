# Sprint 119 Integration Verification

## Retained first committed attempt

Head `712e3cc5eae19170601d3c3feaee4deab03bbbd4`:

- Local Windows workspace Cargo tests: 1,126 passed / 6 intentional ignores.
- Affected suites: bench 78/3 ignored, CLI unit 310/0, CLI integration 68/0,
  bench command integration 7/0, shared process 6/1 ignored, source ratchet 1/0,
  template hygiene 3/0. Real Python grading was explicitly enabled.
- [CI run 33934904691](https://github.com/crussella0129/Animus_Ferric/actions/runs/33934904691):
  **failure** overall; five jobs succeeded and isolated Linux lifecycle failed.
  Both platform workspace jobs, both feature lint matrices, Windows native
  lifecycle, and aarch64 compile checks passed within those jobs.
- Linux workspace execution explicitly passed parent watcher, invalid pidfd
  events, stale registration generation, serialized shutdown, inherited writer,
  timeout/success/unwind and bounded-capture assertions. These are actual Linux
  tests, not the aarch64 compile-only gate.

This head is superseded by the documented
[Test corrections](../test-phase-corrections.md); partial green gates do not
accept E04/E05/E07 while positive Linux lifecycle fails. Corrected-head native
CI and final clause confirmation must be recorded before Test acceptance.

## Corrected intermediate head

Head `0776b6d986ba3852aba14b1742926a9a90343f9d` passed all six jobs in
[CI run 33935599036](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935599036):
Windows/Linux workspace fmt/clippy/tests, backend-openai clippy, Windows/Linux
native lifecycle, and aarch64 workspace/lifecycle compile checks. Native Linux
lifecycle passed 6/6 in 5.73s; Windows passed 5/5 in 19.23s. Exact command names,
per-suite confirmations and native clause test names remain in those job logs.
This is positive evidence for the live supervisor and admission fence, but the
subsequent deadline-ordering correction requires final head `81c9aea` CI too.

## Pre-extra-audit acceptance evidence (superseded)

Head **`81c9aeaf0a9c08f8909395d77a6c7bd53204ee94`** was pushed and confirmed
by `git ls-remote`. [CI run 33935893263](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263)
completed **success, all six jobs**. At initial Test acceptance only Book
evidence had changed after that head. The extra audit later rejected a remaining
deadline defect, so these results remain historical and cannot accept its
subsequent source correction.

| Required gate | Authoritative confirmation |
|---------------|----------------------------|
| Linux workspace fmt/clippy/tests | [job 101223644887](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263/job/101223644887): success; full per-suite Cargo confirmations retained in job log |
| Windows workspace fmt/clippy/tests | [job 101223644987](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263/job/101223644987): success; full per-suite Cargo confirmations retained in job log |
| backend-openai all-target clippy | [job 101223644834](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263/job/101223644834): success, warnings denied |
| Linux lifecycle-feature lint + isolated Cargo lifecycle | [job 101223644911](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263/job/101223644911): success, 6/6 tests in 5.32s |
| Windows lifecycle-feature lint + native Cargo lifecycle | [job 101223644934](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263/job/101223644934): success, 5/5 tests in 19.10s |
| aarch64 workspace + lifecycle-feature compile checks | [job 101223644795](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263/job/101223644795): success; compile-only, not a native aarch64 runtime claim |

Local final-head Windows verification independently passed 1,128 tests with
6 intentional ignores, plus fmt and all three warnings-denied clippy matrices,
as detailed in the unit record. The E01-E06 assertion map is in that record;
E07 combines the static ratchet with actual native CI execution. E08 remains
the documented mandatory Loop/offer-for-merge audit, not a waived condition.

## Final post-audit source-head evidence

**Source `1d877c1858f1eae73716132cf2ae1a5d1a587eb9`: all six jobs passed** in
[CI run 33937071734](https://github.com/crussella0129/Animus_Ferric/actions/runs/33937071734).
The run's exact `headSha`, completed status and success conclusion were checked
after completion, not inferred from the previous source run.

| Required gate | Successful job |
|---------------|----------------|
| Linux workspace fmt/clippy/tests | [101226962804](https://github.com/crussella0129/Animus_Ferric/actions/runs/33937071734/job/101226962804) |
| Windows workspace fmt/clippy/tests | [101226962735](https://github.com/crussella0129/Animus_Ferric/actions/runs/33937071734/job/101226962735) |
| backend-openai all-target clippy | [101226962771](https://github.com/crussella0129/Animus_Ferric/actions/runs/33937071734/job/101226962771) |
| Linux lifecycle-feature lint + isolated Cargo lifecycle | [101226962739](https://github.com/crussella0129/Animus_Ferric/actions/runs/33937071734/job/101226962739), 6/6 in 4.89s |
| Windows lifecycle-feature lint + native Cargo lifecycle | [101226962716](https://github.com/crussella0129/Animus_Ferric/actions/runs/33937071734/job/101226962716), 5/5 in 19.76s |
| aarch64 workspace + lifecycle-feature compile checks | [101226962635](https://github.com/crussella0129/Animus_Ferric/actions/runs/33937071734/job/101226962635), compile-only |

Local source verification independently passed 1,129 Windows workspace tests,
6 intentional ignores, fmt and workspace all-target warnings-denied clippy.
Named E01/E02 deadline assertions supplement the preserved native process
regressions; per-suite CI logs retain the actual confirmations. The new evidence
supersedes `81c9aea` acceptance after the failed extra audit, without rewriting
that audit or pretending its original green CI proved the uncovered edge case.
