# Sprint 121 corrected-source canonical CI — passed

[Run 34004554100](https://github.com/crussella0129/Animus_Ferric/actions/runs/34004554100),
push attempt 1, exact head `a417c5d00361fd25a238346e5015fb07ed5ae7c7`.
Created 2026-09-06T01:41:43Z; completed by 01:49:14Z. Authoritative conclusion:
**success**, all eight jobs. No rerun, cancellation or dispatch was requested.

| Job | Job ID | Actual result |
|---|---|---|
| Windows workspace fmt/Clippy/test | 101409291183 | 1,303 passed, zero failed, thirteen documented ignores. |
| Isolated Linux workspace fmt/Clippy/test | 101409291169 | 1,309 passed, zero failed, nine documented ignores. |
| Windows backend-free CLI | 101409291135 | 416 passed, zero failures/ignores. |
| Linux backend-free CLI | 101409291123 | 416 passed, zero failures/ignores. |
| Windows native lifecycle | 101409291142 | Five passed, zero failures/ignores. |
| Linux native lifecycle | 101409291168 | Six passed, zero failures/ignores. |
| Backend OpenAI Clippy | 101409291037 | Passed; lint, not runtime evidence. |
| ARM64 Linux checks | 101409291118 | Passed; default workspace and lifecycle cross-compilation, not native execution. |

An independent read-only observer inspected all completed authoritative job
logs. Both platforms passed all four new reset/fatal/deadline regressions
named in [the correction record](windows-reset-correction.md). The original
`human_first_run_decision_budget` passed at 01:45:24.2642902Z on Windows and
01:44:38.7911313Z on Linux. No inspection-failure, failing fixture/session or
panic block appeared. The historical uninstrumented failure's cause remains
unknown; its original failure and the intermediate non-reproduction are not
rewritten by this corrected-source result.

## Canonical affected-suite confirmations

Both platforms: benchmark library 97/four source-mode ignores; bench-budget
integration ten; existing bench-mock seven; budget-docs one; CLI integrations
75; human CLI eight; human docs one; query-budget HTTP three; loop-output-budget
five; source-execution two; template hygiene three. Windows CLI units passed
393/six ignores; Linux 394/four. Deterministic stalled-phase cleanup, all four
identity-hash tests and stage-journal retention passed on both. The unchanged
actual Python grading tests reported success, separate from the synthetic
Ferric-as-Python argv fixture used by diagnostic CLI publication tests.

Backend-free 416 on each host comprises 320 units, three bench-budget, seven
bench-mock, one budget-doc, 73 CLI, six human-CLI, one human-doc, two source and
three hygiene tests. Backend-specific cases are excluded, not counted as runs.
Lifecycle jobs passed five shared adoption/ownership/LocalAPI safety cases;
Linux additionally passed `lifecycle_fixture_exits_when_exact_owner_pidfd_signals`.

Actual GGUF smoke tests and Windows diagnostic-series modes are intentionally
opt-in in normal CI. The [fresh corrected-source local smoke](e2e-tests.md#corrected-source-live-test-002--passed)
is separate required evidence and passed with checked cleanup. Root also
retained all 79 suite confirmations from the same-source [native Windows run](windows-source-a417c5d.txt).
Neither native isolated Linux CI nor the small [WSL core run](wsl-checkpoint-001.md)
claims ordinary-host Linux ownership or macOS parity.

This completes the Test-stage canonical matrix. Final independent critique and
report, Loop reconciliation, the extra post-Loop adversarial audit and the
sole PR's final-head checks remain separate gates in that order.
