# Sprint 119 Unit Verification

## Retained Build attempts

These attempts ran against the evolving working tree; they are not a substitute
for the final committed-head gates recorded below at Test closeout.

| Platform | Command | Result | Meaning |
|----------|---------|--------|---------|
| Windows | `cargo check -p ferric-process --lib --offline` | pass | Shared native library type-check. |
| Windows | `cargo check -p ferric-cli --features lifecycle-fixture --all-targets --offline` | pass, 12.13s | Initial consumer/API integration type-check; later edits require rerun. |
| Windows | `cargo test -p ferric-process --locked` (first) | **fail: 4 passed, 1 failed, 1 ignored** | `leader_exit_reaps_descendants` observed WAIT_TIMEOUT from a retained process immediately after cleanup success. Job active-count zero was insufficient by itself. |
| Windows | `cargo test -p ferric-process --locked leader_exit_reaps_descendants` (after native fix) | pass: 1 passed, 5 filtered, 0.14s | Cleanup retains Job member handles and waits for termination within its existing deadline. The test gained identity labels, not a grace period. |
| Windows | `cargo test -p ferric-process --locked` (after native fix) | pass: 5 passed, 1 ignored, 0.61s; 0 doc tests | E01/E02/E03 and registry unit coverage; fixture ignore is recursively exercised by source tests. |

The first failing run remains failed. No manual cleanup or direct artifact
invocation was used. A later independent review also identified an unlocked
leader-only signal race with Linux shutdown/reaping; final gates must include
its correction and must not reuse the intermediate green matrix blindly.

Further Build checks: shared Windows suite passed 6 tests with one recursive
fixture ignored after generation-tagged registration and thread-owner checks.
Windows and aarch64 shared all-target clippy passed with warnings denied.
The first consumer lifecycle-feature clippy attempt failed on an unused
Linux-only `File` import on Windows; a cfg-qualified import corrected it.
Workspace clippy, backend-openai clippy, lifecycle-feature clippy, and fmt then
passed. Windows workspace Cargo tests passed, including benchmark 78/3 ignored,
CLI units 310/0, CLI integration 68/0, bench_mock 7/0, source ratchet 1/0,
template hygiene 3/0, and shared process 6/1 ignored. The separate serialized
lifecycle-feature suite passed 5/5 in 19.72s. Benchmark Python checks used an
explicit available interpreter through `FERRIC_TEST_PYTHON`, not skipped probes.
