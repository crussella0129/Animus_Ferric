# Sprint 121 instrumented CI checkpoint — non-reproduction

[Run 34003898449](https://github.com/crussella0129/Animus_Ferric/actions/runs/34003898449),
push attempt 1, exact source `4eded51eaa6e0681f513ae5f8a1891de841a5a8b`.
Created 2026-09-06T01:26:34Z; completed/updated 01:34:14Z. Authoritative
conclusion: **success**, all eight jobs. This is an instrumented
non-reproduction, not a demonstrated repair or explanation of original failed
run 34002834811. No workflow rerun, dispatch or cancellation was requested.

| Job | Job ID | Actual result |
|---|---|---|
| Windows workspace | 101407530116 | 1,299 passed, thirteen documented ignores. |
| Linux workspace | 101407530021 | 1,305 passed, nine documented ignores. |
| Windows backend-free | 101407530124 | 416 passed, no ignores. |
| Linux backend-free | 101407530171 | 416 passed, no ignores. |
| Windows lifecycle | 101407530118 | Five passed, no ignores. |
| Linux lifecycle | 101407530154 | Six passed, no ignores. |
| Backend OpenAI Clippy | 101407530086 | Passed. |
| ARM64 compilation | 101407530121 | Passed. |

Counts came from authoritative job logs in an independent read-only check.
Both workspace formatting checks and warnings-denied Clippy passed. Windows
CLI units passed 389 tests with six ignores; Linux passed 390 with four. The
two extra Windows ignores are the source-supervised diagnostic parent and
child, not skipped acceptance tests. All ten benchmark-budget integrations,
three query HTTP cap tests, five loop output-budget tests, expert-documentation
ratchet, deterministic stalled phases, four identity-hash tests and stage
journal passed on both platforms.

The original first-run journey passed at 01:30:24.1460459Z on Windows and
01:29:25.5662891Z on Linux. Neither completed workspace log contained an
`OWNED_ENGINE_INSPECTION_FAILURE`, failing fixture/session record or panic
block. Those absences cannot explain the earlier uninstrumented failure.

Independent Test review still returned `block`: the review identified a real
fixture defect in connection-reset handling and requires its meaningful
regressions, corrected-source qualification and renewed critique before the
report. That correction postdates this checkpoint. The failed original matrix
and both diagnostic non-reproductions remain retained; no retrospective green
or production native-ownership fix is claimed.
