# Sprint 121 Build verification (not final Test acceptance)

## T-12101 candidate and corrections

Source-aware Cargo commands only; the worktree above locked-plan commit
`b72ae0f` is not yet an immutable accepted implementation head.

- `cargo check -p ferric-cli --all-targets --locked` initially failed on the
  exhaustive `trace_verify` event match. Added the new main-action record to
  active, unfinished-turn validation and the human trace renderer; existing
  events and old records remain supported.
- First core/trace/loop and CLI test compilations failed for zero free disk
  bytes. The scoped regenerable cache recovery is recorded in sprint metadata.
  These were compiler failures, not runtime results or successful test runs.
- With `CARGO_INCREMENTAL=0`, the first executed core/trace/loop run reached
  the old `happy_path_golden_trace_order` assertion and failed because its exact
  expected sequence omitted the two new main-action records. Added both
  expected records while retaining the exact sequence/monotonic assertions.
- The first executed CLI suite reported 74 passed / 1 failed. The generated
  resume command's unquoted numeric values were captured by the actual
  PowerShell function as JSON numbers, not strings. Generated cap/context now
  use the same documented-shell quoting as other argument values. This preserves
  their native command values and makes the supported shell round-trip exact.
  The requested wire fixture had not run after that suite stopped.

Supplementary read-only review found no production blocker in admission,
omitted defaults, authority, compaction independence or additive recovery.
Its optional stale explicit-metadata direct-caller case was added: actual
sampler 777 must clear stale requested 4096 and record source `caller`.
This reviewer authored the resume fixtures; this supplementary review does
not replace independent final Test or post-Loop acceptance.

## T-12101 corrected task boundary

The corrected core/trace/loop suite passed 311 tests with no failures or ignores.
The next CLI run passed all 75 CLI tests, but the HTTP wire fixture exited 125
without retaining its original worker panic payload. Isolated diagnostic runs
passed; those samples did not establish acceptance or explain that historical
exit's exact stage.

Supplementary review identified a real Windows fixture defect: accepted sockets
inherit the listener's properties, so the nonblocking listener required an
explicit blocking mode on each accepted stream before bounded framed reads.
See Microsoft's [Winsock accept contract](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept).
The joined-worker panic path also incorrectly treated a proved-joined worker
failure as unproved cleanup. The fixture now resumes the original panic after
joining; an actual join deadline failure remains a cleanup failure.

Added `http_budget_fixture_waits_for_fragmented_request`: wait for acceptance,
delay the first bytes, then fragment the request with bounded I/O. The corrected
three-test fixture suite passed. A temporary negative-control mutation removed
only the explicit blocking-mode reset: this new test failed as expected with
Winsock 10035 (`WouldBlock`) in the worker and connection-aborted in the client.
The reset was immediately restored. This proves the regression detects the
socket-mode defect; it does not retroactively prove the hidden exit-125 cause.

Final coherent task gate on 2026-09-05:

- `cargo fmt --all --check`: passed.
- `CARGO_INCREMENTAL=0 cargo clippy -p ferric-core -p ferric-trace -p ferric-loop -p ferric-cli --all-targets --locked -- -D warnings`: passed.
- `CARGO_INCREMENTAL=0 cargo test -p ferric-cli --test query_output_budget --test cli --locked -- --test-threads=1`: 75 CLI plus three HTTP fixture tests passed, no failures or ignores.
- Earlier corrected `cargo test` core/trace/loop result remains 311 passed;
  subsequent corrections touched only the CLI fixture.
- `git diff --check`: passed. Cargo's existing duplicate-target warning remains
  separately tracked as T-12028; no build profile or assertion was relaxed.

E01-A/B/C are covered by the named resolver, rejection-before-effects, actual
stream/nonstream wire and trace, direct-caller provenance, legacy metadata and
actual shell/resume tests in the locked Test plan. E01-D/E are covered by the
five named loop tests (24 internal matrix cases) for exact 24 KiB publication,
default/forced truncation, unchanged authority and independent compaction.
All launched test children use the existing source-owned checked process
contract; HTTP workers are finite and joined. No manual process repair occurred.
This completes the task boundary, not formal Test/Loop or model acceptance.
Confidence remains unchanged pending the complete sprint evidence.

## T-12102 integration candidate

T-12101 is now committed at `8be0e9f`, with reachable ledger evidence in
`ddf0aed`. T-12102 continues the same locked sprint, without another Plan or PR.

Checked parent deadlines are pre-resolved for all selected specs before
preflight, then reused across rotated trials. Default scale/no cap preserves
the legacy child argv; explicit controls propagate validated context/parameters
to both real and mock children. Parent declarations remain distinct from actual
observed main-request metadata. Rows, versioned sidecars and summary references
bind retained trace bytes rather than adding parent events to child traces.

Read-only Build review identified and closed three reporting gaps before the
first integration gate: malformed known budget vocabulary must be
infrastructure-failed despite the generic reader's unknown-event compatibility;
row budget/metrics must use verified retained evidence; and successful task
behavior must not print an unqualified PASS after retention or row-append
failure. This review is not final Test or post-Loop acceptance.

Initial source-aware gates on 2026-09-06:

- `CARGO_INCREMENTAL=0 cargo check -p ferric-cli --all-targets --locked`: passed.
- `CARGO_INCREMENTAL=0 cargo test -p ferric-cli --test bench_budget --test bench_mock --locked -- --test-threads=1`: eight new tests plus seven existing benchmark integrations passed, no failures or ignores.
- New integration cases cover twelve invalid inputs before Python preflight,
  omitted/explicit-1 legacy mock defaults, accepted mock context/cap, actual
  HTTP requests and row/summary/trace/sidecar readback, distinct provider-error
  and output-limit stops, a six-second parent timeout after a real request,
  and successful scripted L0 outcomes with retention-only or append-only
  infrastructure failures. HTTP workers were finite/joined and all children
  returned through the checked source process owner.

Library arithmetic/argv/publication fault matrices and the final coherent
formatter/linter/task gates remain pending. No calibration, live model,
formal Test or Loop acceptance is implied.

## T-12102 completed task boundary

Subsequent coherent gates passed on 2026-09-06:

- `cargo fmt --all` followed by `cargo fmt --all --check`: passed.
- `CARGO_INCREMENTAL=0 cargo clippy -p ferric-bench -p ferric-cli --all-targets --locked -- -D warnings`: passed.
- `CARGO_INCREMENTAL=0 cargo test -p ferric-bench --locked -- --test-threads=1`: 94 passed, zero failures, four ignored source child entries. Those entries are deliberately entered through checked parent tests: the existing noisy/check modes and the new finite budget child; they are not missing acceptance scenarios.
- `CARGO_INCREMENTAL=0 cargo test -p ferric-cli --bin ferric autonomy_cmd::tests --locked -- --test-threads=1`: 26 passed, zero failures or ignores (356 unrelated units filtered).
- The eight new and seven existing CLI integration results above remain the
  composed source run; subsequent changes added library tests and formatting,
  not a different production behavior.

Named E02-A arithmetic/preflight, E02-B real/mock/resume/frozen argv,
E02-C/D missing/malformed/legacy attribution, byte/digest/identity readback,
no-clobber collisions and injected partial sidecar writes all passed. E02-E's
two-second source-owner fixtures prove checked cleanup with no trace and noisy
partial trace; the six-second composed HTTP timeout separately exercises the
actual benchmark runner. Grader/process/spec sources are unchanged. Parent
cleanup completed before every successful runner return; no manual repair.

A separate read-only reviewer inspected all E02 clauses and their named source
coverage and returned no remaining blocking findings. This is T-12102 Build
readiness only: the diagnostic calibration guard is intentionally the next
ordered task, and formal Test, live smoke, Loop and PR remain outstanding.
