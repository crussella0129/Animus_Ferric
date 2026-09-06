# Test correction: connection-scoped human fixture resets

The independent blocking Test critique identified a concrete fixture defect:
an incoming queued peer can reset before `accept`, but the source fake engine
treated every non-WouldBlock accept error as normal engine return. Microsoft's
[accept contract](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept)
defines WSAECONNRESET for this peer event. The listener remains usable. This
justifies a narrow fixture correction independently of the **still unknown
cause** of original CI failure 34002834811. Diagnostic checkpoint 34003898449
passed before this correction and is not represented as its proof.

## Correction and assertions

Only the source-included human test fixture changed. `fixture_accept_next`
polls for the existing WouldBlock case and the newly classified ConnectionReset;
all other unexpected errors remain fatal. It retains the same absolute
45-second engine lifetime and ten-millisecond polling, caps the final pause at
the remaining time, and refuses an accepted socket observed after the deadline.
No production process inspection retry, native authority change, dependency,
CI schedule adjustment or deadline increase was made.

Four new tests execute the shared branch, not a parallel classifier:

- `human_first_run_accept_reset_preserves_decisions_answer_and_cleanup` injects
  the actual Windows 10054 code (typed reset on Linux), records a create-new
  marker only after the branch classified it, then serves the actual prepared
  first-run conversation. It reuses every original three-decision, answer,
  no-write, no-technical-question and checked listener-closure assertion.
- `human_first_run_fatal_accept_error_refuses_and_reaps` observes the delivered
  fatal fault, requires refusal with no Ready/answer/write, and proves listener
  closure plus workspace-lock reacquisition.
- `fixture_accept_fatal_error_is_not_retried` covers five fatal kinds and
  asserts one attempt, no pause, exact error and non-retry observation.
- `fixture_accept_repeated_resets_keep_absolute_deadline` uses a deterministic
  clock to prove repeated reset and WouldBlock polling exhaust the same bound,
  with no accept/sleep/error observation after expiration.

The formatted corrected fixture SHA-256 is
`2d6cffe9a14c62d436521a05dee29d5cc53f9408b30352b95401756315bbfcdc`.
Warnings-denied CLI all-target Clippy passed. The human source group passed
21 tests in 2.94 s, with three explicit opt-in ignores (diagnostic parent,
diagnostic child, existing real-model journey). [Actual confirmations](windows-reset-positive.txt)
retain every test name; this is a task-local candidate result, not final CI.

## Meaningful mutation negative control

Root temporarily removed only ConnectionReset from the shared retry match,
restoring the old decision while keeping the new injected journey and all
assertions. Exactly one Cargo test then failed as required: zero passed, one
failed, 0.13 s, command exit 1. [Raw output](windows-reset-negative-control.txt)
records the observed `ConnectionReset`/10054 at 1 ms, fatal accept classification,
source engine return at 10 ms, preparation failure at 104 ms and session failure
at 125 ms. The source fixture's own test returned normally while the actual
parent journey correctly failed before Ready or an answer. The session's
`closed_listener_check` was true after checked owned cleanup; no manual process
termination occurred.

The exact correction was restored immediately. Its SHA-256 matched the value
above and `git diff --check` passed. This proves the regression detects the
identified fixture defect. It does not prove that the historical uninstrumented
CI failure followed the injected path; its error timing/message may differ.
Fresh immutable corrected-source gates and renewed independent critique remain
required before acceptance.
