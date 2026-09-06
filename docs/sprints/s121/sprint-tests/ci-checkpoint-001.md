# Sprint 121 initial immutable CI checkpoint — failed

[Run 34002834811](https://github.com/crussella0129/Animus_Ferric/actions/runs/34002834811),
push attempt 1, source `2856c63209865f69b3d3727f84fd92f63f9dfa51`.
Created 2026-09-06T01:02:04Z; final update 01:06:49Z. Authoritative conclusion:
**failure**. No workflow rerun or cancellation was requested.

| Job | Job ID | Actual result |
|---|---|---|
| Windows workspace | 101404658930 | Failed: CLI units 388 passed, one failed, four ignored; Cargo stopped before CLI integrations/later crates. |
| Linux workspace | 101404658914 | Passed: 1,305 tests, nine documented ignores, exact non-root namespace runner. |
| Windows backend-free | 101404658900 | Passed: 416 tests, no ignores. |
| Linux backend-free | 101404658949 | Passed: 416 tests, no ignores. |
| Windows lifecycle | 101404658855 | Passed: five tests, no ignores. |
| Linux lifecycle | 101404658883 | Passed: six tests, no ignores, including exact-owner pidfd. |
| Backend OpenAI Clippy | 101404658873 | Passed. |
| ARM64 compilation | 101404658798 | Passed: default workspace and lifecycle all-target checks. |

All job logs were read from the authoritative GitHub job-log API. Windows
formatting/included-fixture formatting/warnings-denied Clippy passed before
the test failure. Its complete partial test count is 507 passed, one failed,
eight ignored: launch ten, scaffold twelve, bench 97/four ignored and CLI
388/one failed/four ignored. Seven successful jobs do not override this failure.

## Exact failure evidence

```text
2026-09-06T01:05:49.5447469Z test human::enabled::tests::human_failure_is_concise ... ok
2026-09-06T01:05:49.6030303Z test human::enabled::tests::human_first_run_decision_budget ... FAILED
2026-09-06T01:05:50.3783207Z test human::enabled::tests::human_journey_e2e_matrix ... ok
2026-09-06T01:06:01.7130678Z test live_budget_tests::live_budget_fixture_stalled_phases_reap ... ok
2026-09-06T01:06:34.3180373Z ---- human::enabled::tests::human_first_run_decision_budget stdout ----
2026-09-06T01:06:34.3181526Z thread 'human::enabled::tests::human_first_run_decision_budget' (6404) panicked at crates\ferric-cli\src\human_journey_tests.rs:330:12:
2026-09-06T01:06:34.3183113Z called `Result::unwrap()` on an `Err` value: "The owned engine exited or cannot be verified. Inspect the model and server configuration for the selected folder."
2026-09-06T01:06:34.3186539Z test result: FAILED. 388 passed; 1 failed; 4 ignored; 0 measured; 0 filtered out; finished in 49.62s
2026-09-06T01:06:34.4551530Z ##[error]Process completed with exit code 1.
```

The original log has no separate engine/stage/native-error detail explaining
this result. `OwnedEngine::listener` mapped a retained `LiveProcess::inspect`
error to the displayed message. Listener ownership mismatch has a different
message; TCP-table failure is an uninspectable listener rather than this
inspection error. The original native error is discarded, and the human
renderer deliberately omits bounded engine tails from concise operator text.
The fixture did not print the full error or captured scripted session.

The new live-budget stalled fixture runs after the failed human journey, not
before it. Its required two four-second outer timeouts also rule out casually
attributing the failure to the human engine's 45-second lifetime within this
49.62-second serialized binary. No longer deadline, inspection retry, fixture
skip, native authority expansion or speculative cause is accepted.

The same immutable source passed root's full Windows run (1,299 tests, eleven
documented ignores), and its fresh release live smoke passed. Those independent
observations do not erase this checkpoint failure. Test remains unaccepted,
and no Test pass report, Loop close or PR may treat the failed matrix as green.
