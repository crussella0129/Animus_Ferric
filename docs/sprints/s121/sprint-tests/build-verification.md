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
