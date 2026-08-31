# Sprint 117 Test Report

## Verdict

**Pass for Sprint 117's lifecycle acceptance-recovery scope.** All nineteen
frozen EARS commands passed at corrected immutable implementation head
`b679a25ba83069ab849b0f7f2eb8a3269eba10c5`; the independent final
[test critique](critique.md) is `clean`; and authoritative CI run
[33351978700](https://github.com/crussella0129/Animus_Ferric/actions/runs/33351978700)
passed all six jobs.

This closes the clause-level evidence gap left by Sprint 116 for the affected
server-lifecycle portions of INT-0008 AC-3, AC-4, AC-6, and AC-7. It provides
enabling evidence toward AC-9 but does not complete the wider compact local-
model workflow, cross-platform AC-8, macOS support, or an actual model-backed
application trial. INT-0008 therefore remains `active`, not `realized`.

## Acceptance result

| Clause / task | Result | Exact command evidence |
|---|---|---|
| T-11701 / E01-A | pass | [`cargo test -p ferric-cli --all-features registration_inventory_retains_both_scopes_and_raw_bytes -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed. |
| T-11701 / E01-B | pass | [`cargo test -p ferric-cli --all-features runfile_schema_authority_matrix -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed. |
| T-11701 / E01-C | pass | [`cargo test -p ferric-cli --all-features concurrent_lifecycle_operations_are_per_path_safe -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11701 / E01-D | pass | [`cargo test -p ferric-cli --all-features atomic_conditional_removal_matrix -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11702 / E02-A | pass | [`cargo test -p ferric-cli --all-features retained_process_handle_identity_matrix -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [native-platform corroboration](integration-tests.md#native-platform-smokes). |
| T-11702 / E02-B | pass | [`cargo test -p ferric-cli --all-features loopback_listener_ownership_matrix -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [native-platform corroboration](integration-tests.md#native-platform-smokes). |
| T-11702 / E02-C | pass | [`cargo test -p ferric-cli --all-features spawned_child_binding_window_matrix -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11703 / E03-A | pass | [`cargo test -p ferric-cli --all-features registration_resolution_cross_workspace_matrix -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11703 / E03-B | pass | [`cargo test -p ferric-cli --all-features status_reports_scope_identity_health_and_next_action -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11703 / E03-C | pass | [`cargo test -p ferric-cli --all-features registration_consumers_propagate_typed_ambiguity -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11704 / E04-A | pass | [`cargo test -p ferric-cli --all-features down_signals_only_the_retained_handle -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11704 / E04-B | pass | [`cargo test -p ferric-cli --all-features down_exit_and_listener_postconditions_gate_success -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11704 / E04-C | pass | [`cargo test -p ferric-cli --all-features down_cleanup_outcome_matrix -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11704 / E04-D | pass | [`cargo test -p ferric-cli --all-features ambiguous_or_unverifiable_down_is_non_mutating -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11704 / E04-E | pass | [`cargo test -p ferric-cli --all-features live_v1_guidance_and_explicit_adoption -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [CLI and adoption corroboration](e2e-tests.md#supplemental-cli-lifecycle--e04-ee04-a). |
| T-11705 / E05-A | pass | [`cargo test -p ferric-cli --all-features registration_publication_is_complete_synced_and_no_clobber -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11705 / E05-B | pass | [`cargo test -p ferric-cli --all-features partial_publication_stops_child_and_compensates_exactly -- --nocapture`](unit-tests.md#frozen-clause-matrices) — exactly 1 passed; [integration corroboration](integration-tests.md#registration-and-lifecycle-composition). |
| T-11706 / E05-C | pass | [`cargo test -p ferric-cli --all-features model_free_server_lifecycle_fixture_e2e -- --nocapture`](e2e-tests.md#frozen-end-to-end-rows) — exactly 1 passed. |
| T-11705 / E05-D | pass | [`cargo test -p ferric-cli --all-features tailscale_mode_refuses_before_side_effects -- --nocapture`](e2e-tests.md#frozen-end-to-end-rows) — exactly 1 passed. |

Detailed command and assertion provenance is retained in
[unit/static](unit-tests.md), [integration](integration-tests.md), and
[end-to-end](e2e-tests.md) evidence.

## Canonical confirmations

- Both frozen Presence commands passed and exposed all nineteen names.
- Each frozen clause filter passed exactly one intended test.
- `cargo test -p ferric-cli --all-features --locked`: 336 passed, 0 failed.
- `cargo test --workspace --all-features --locked`: 1,089 passed, 0 failed,
  4 intentional ignores.
- The serialized lifecycle fixture passed 3/3 locally; CI repeated 3/3 on
  Windows and in an isolated non-root, capability-free Ubuntu payload.
- Frozen fmt/Clippy gates, strict workspace Clippy, both AArch64 compile
  checks, and `git diff --check` passed.
- The protected Sprint 114 acquisition artifact remained unstaged with
  SHA-256
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`.

## Reliability and correction record

The failed Ubuntu lifecycle attempt at `6eab44b` exposed an honest authority
boundary: ordinary host-namespace `/proc` enumeration could not inspect PID 1,
so Ferric correctly refused positive ownership. The accepted fixture prepares
an isolated PID/network/proc namespace, then runs the payload as the ordinary
runner UID/GID with no capabilities. Production authority was not weakened.

Before the final implementation commit, default-parallel Windows testing also
exposed a flaky stale-numeric-HANDLE test oracle. The proof now checks the real
`CloseHandle` result through the same close path used by `Drop`, followed by
eight consecutive 255/255 parallel runs, full local suites, and green Windows
CI. Both failed attempts remain recorded rather than being hidden by the final
pass.

The pre-merge adversarial review then found a real publication-boundary gap:
the successful path checked the child after persistence but did not re-inspect
the retained generation and exclusive listener ownership before returning
`Ready`. Commit `b679a25` adds that post-publication authority gate and exact
identity-transition/listener-transition compensation rows. The same review
also found that this report compressed the frozen API into six aggregate rows
instead of the required nineteen-row ledger. The implementation, all nineteen
frozen filters, all supplemental filters, package/workspace suites, native
fixture, cross-target checks, and both six-job CI triggers were requalified at
the corrected head.

## Evidence boundary and carry-forward

- Positive Linux lifecycle E2E is proved only where every relevant namespace
  peer is visible. Incomplete ordinary-host peer visibility remains
  non-authorizing and has a passing negative regression.
- AArch64 is compile-only evidence; there is no AArch64 runtime result.
- No macOS lifecycle parity is claimed.
- The fixture uses a closed-name Rust server and proves lifecycle semantics,
  not inference quality, GGUF compatibility, calibration, or the deferred
  model-backed medium-horizon application task.
- T-11606 is closed by this sprint; the broader local-model work continues in
  the ordered backlog, whose later tasks still own bounded calibration,
  runtime discovery, reasoning and compaction controls, and the small human
  command surface.
