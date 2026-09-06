# Windows first-run diagnostic checkpoint

This is diagnosis of failed CI run 34002834811, not acceptance or a claim that
the failure was fixed. The original source is `2856c63209865f69b3d3727f84fd92f63f9dfa51`.
The candidate adds test-only native-error/session/engine-stage diagnostics and
a Windows-only ignored source-supervised diagnostic series. Production error
text, identity checks, socket behavior, deadlines and cleanup are unchanged.

The initial diagnostic compile failed: `BoundedOutcome.exit_code` is
`Option<i32>`, but the new assertion compared it with `0`. It was corrected to
`Some(0)`. No test executed in that failed command; it is not a runtime failure
or pass. The corrected warnings-denied CLI all-target Clippy passed.

Executed once through source-aware Cargo:

```powershell
$env:CARGO_INCREMENTAL='0'
cargo test -p ferric-cli --bin ferric human::enabled::tests::human_first_run_diagnostic_series --locked -- --ignored --exact --test-threads=1 --nocapture
```

The fixed sample passed one parent test in 6.70 s. Its source child ran the
original `human_first_run_decision_budget` assertions exactly 32 times in
sequence (completion markers 1 through 32), with no retry/reset after a
failure. Child elapsed 6,616 ms; parent execution 6,691 ms; spawn 44 ms. Parent
exit code was 0, execution timeout false, checked process-scope cleanup true;
stderr was empty. The parent retained its 60-second execution and existing
five-second cleanup budgets, and each original journey retained its own
listener-closed assertion. No engine was manually stopped.

This did not reproduce the CI failure and does not supply its root cause.
An additional bounded engine-tail snapshot was subsequently added to the
test-only inspection-error record so failures after preparation also preserve
available child diagnostics. That snapshot is not a claim that the log reader
had finished; checked cleanup and preparation's post-cleanup error record
remain separate evidence. The sample above predates this extra record field.

Read-only source inspection confirms that the process HANDLE is duplicated
from the exact spawned Child, retained through the session, and checked active
before and after TCP inventory. No evidenced identity-reuse fix follows from
the original error. The fixture can return early on an accept error other than
WouldBlock; that is only a hypothesis until a failing sample records it.

Next qualification may use the instrumented source to collect missing failure
details, retaining both the failed original checkpoint and this non-reproduction.
A green diagnostic run alone must not be called a repair or override the
locked final-head qualification requirement.

After the tail-snapshot addition, workspace and included-fixture formatting,
warnings-denied CLI all-target Clippy, and `git diff --check` passed.
`cargo test -p ferric-cli --bin ferric human::enabled::tests --locked -- --test-threads=1`
passed 17 tests in 2.56 s. Three entries were explicitly ignored: the two
opt-in diagnostic parent/child modes and the existing live-model journey.
This includes the actual first-run test, composed journey matrix, request
cancellation, scoped consent and exact listener-closure assertions.
