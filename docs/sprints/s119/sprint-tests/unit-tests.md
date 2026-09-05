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

## First committed Test head (superseded)

Head `712e3cc5eae19170601d3c3feaee4deab03bbbd4` was pushed and independently
confirmed with `git ls-remote`. Local Windows `cargo test --workspace --locked
--offline --quiet` passed **1,126 tests, 6 intentional ignores**, with the real
test Python selected explicitly. The separate final-head fmt, workspace,
backend-openai and lifecycle-feature warnings-denied clippy checks passed.

[CI run 33934904691](https://github.com/crussella0129/Animus_Ferric/actions/runs/33934904691)
passed Linux workspace tests, backend-openai clippy, aarch64 compile checks,
and the Windows native lifecycle job, but failed the isolated Linux lifecycle
suite **3 passed / 3 failed**, exit 101. The retained unreaped launcher was a
zombie; complete `/proc` listener-owner enumeration correctly refused its
unreadable fd directory. The failing tests were
`model_free_server_lifecycle_fixture_e2e`,
`tailscale_localapi_lifecycle_preserves_unrelated_state`, and
`tailscale_localapi_log_contains_no_broad_mutation_or_retry`.
The exact-owner-death regression passed. Production ownership checks must not
be relaxed to hide the fixture topology error.

The same head's final source review found that a Windows child born after the
member-handle snapshot could escape that snapshot's termination proof.
Thus even its passing Windows tests are **not final acceptance**. A counted
membership fence and deterministic late-birth regression are required before
accepting E01; the first stable-descendant regression alone was insufficient.

## Locked clause assertion map

All rows link to INT-0008 AC-6; E04/E05 additionally provide model-free enabling
AC-9 evidence only. This is an assertion map, not a final pass verdict.

| EARS | Named source assertions | What a pass proves |
|------|-------------------------|--------------------|
| E01 | `scope_cleanup_success_timeout_unwind`, `leader_exit_reaps_descendants`, `windows_cleanup_rejects_post_snapshot_admission`, `windows_cleanup_deadline_precedes_success` | Success/timeout/unwind return is followed by immediate exact native termination/reaping checks; inherited writers do not keep capture alive; late admission refuses the incomplete inner proof and independently drains the outer source scope; expired final observations cannot be accepted as cleanup success. |
| E02 | `platform::windows_spawn_failure_rolls_back` | Real suspended children fail at both BeforeAssign and BeforeResume; retained native handles are signalled on return inside the cleanup bound. |
| E03 | `bounded_capture_head_tail_and_noisy_child`, `verbose_source_child_cannot_deadlock_file_capture`, `command_check_output_is_bounded_in_capture_files`, `command_check_timeout_is_a_model_failure`, `fixed_argv_check_classifies_pass_model_failure_and_infrastructure` | Exact head/tail bytes and sizes, no inherited-writer pipe wait, distinct timeout/exit and model/infrastructure outcomes. |
| E04 | CLI units, `cli` including `mcp_stdio_e2e`, `bench_mock_v2_checks_record_model_failure_with_cargo_fixture`, `two_process_lifecycle_interleaving_is_per_path_safe`, native lifecycle suite | Original command/registration/protocol assertions remain; source scope/file capture owns cleanup, MCP readers join after child cleanup, and intentional managed-server detachment remains usable. |
| E05 | `source_driven_process_tree_regressions`, `parent_watch_retains_identity`, `lifecycle_fixture_exits_when_exact_owner_pidfd_signals`, positive native lifecycle suite | Controlled source owner death stops helpers with exact Windows handles/Linux pidfds; Linux exit-only readiness is insufficient and exact reaping is required; a live supervisor preserves launcher-to-server handoff. |
| E06 | `parent_watch_retains_identity` in CLI/shared modules, `exact_process::invalid_pidfd_events_fail_closed`, shared `supervision::pidfd_event_decoder_distinguishes_exit_reaping_and_invalid`, `shutdown_registry_rejects_late_spawn`, `recycled_id_cannot_redirect_stale_scope_operations` | Parent descriptors stay owned, invalid events fail, shutdown refuses late registrations, final removal is serialized with signal recording, and stale tokens cannot signal/reap/remove replacements. |

E07 and E08 combine integration/E2E execution and the closing audit, documented
in the adjacent result files. Fixture ignores are deliberate recursive source
entry points; the two Windows research ignores are pre-existing platform gates.

## Pre-extra-audit source head (superseded)

`81c9aeaf0a9c08f8909395d77a6c7bd53204ee94` was the initially accepted head,
superseded by the extra post-Loop audit's uncovered Unix deadline defect.
Local Windows source verification at that head:

- `cargo fmt --check`: pass.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  pass (4.58s final repeat).
- `cargo clippy -p ferric-cli --features lifecycle-fixture --all-targets
  --locked --offline -- -D warnings`: pass (4.73s).
- `cargo clippy -p ferric-cli --features backend-openai --all-targets
  --locked --offline -- -D warnings`: pass (7.29s).
- `cargo test --workspace --locked --offline --quiet`: **1,128 passed,
  6 intentional ignores**, across 73 Cargo suite/doc-suite confirmations.
  `FERRIC_TEST_PYTHON` selected a real available interpreter for grading.
  Shared process tests are now 8 passed / 1 source fixture ignored; CLI units
  310, `cli` 68, `bench_mock` 7, bench 78/3 ignored, source ratchet 1, and
  template hygiene 3 all pass.

The independent final source-review verdict is clean after the deadline
correction. Final Test critique remains separate and must consider the complete
CI result and all three evidence artifacts before the report is written.

## Post-audit corrected source verification

Corrected source **`1d877c1858f1eae73716132cf2ae1a5d1a587eb9`** was committed,
pushed and independently matched with `git ls-remote`. Local Windows
`cargo fmt --check` and workspace/all-target locked offline clippy with warnings
denied passed (clippy 4.41s). `cargo test --workspace --locked --offline --quiet`
with an explicit real `FERRIC_TEST_PYTHON` passed **1,129 tests, 0 failures,
6 intentional ignores**, across **73** Cargo suite/doc-suite confirmations.
Shared process tests passed 9/1 ignored, CLI units 310, command integration 68,
bench command integration 7, bench 78/3 ignored, source ratchet 1 and template
hygiene 3. No source test session remained after exit 0.

E01/E02 add `tests::cleanup_deadline_precedes_success`: the exact shared
decision rejects drained and pending observations at or after the deadline.
Source review independently verified its use on both Unix success paths,
Windows Job drain and suspended-child rollback, and lock release before
fail-closed shutdown. The retained native success/timeout/unwind, descendant,
and suspended-spawn rollback assertions still prove actual process cleanup.
Linux CI job `101226962804` explicitly executed the new regression together
with `leader_exit_reaps_descendants` and `scope_cleanup_success_timeout_unwind`;
its shared suite passed **8/1 ignored in 0.17s**. This is native Linux evidence,
not the cross-target compile check. See the final integration matrix for the
complete CI conclusion.
