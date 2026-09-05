Finalized - DO NOT EDIT

# Sprint 119 Test Plan

## Intent Traceability

| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|--------|----------------------|--------------------------|--------------|
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-6 | T-11901 / E01 | `scope_cleanup_success_timeout_unwind`, `leader_exit_reaps_descendants` |
| INT-0008 | AC-6 | T-11901 / E02 | `windows_spawn_failure_rolls_back` |
| INT-0008 | AC-6 | T-11901 / E03 | `bounded_capture_head_tail_and_noisy_child`, benchmark runner/verification regressions |
| INT-0008 | AC-6, AC-9 enabling only | T-11902 / E04 | CLI unit, `cli`, `bench_mock`, and lifecycle fixture suites with all prior assertions retained |
| INT-0008 | AC-6 | T-11902 / E05 | `source_driven_process_tree_regressions`, lifecycle exact-owner-death regression, positive lifecycle fixture suite |
| INT-0008 | AC-6 | T-11902 / E06 | `parent_watch_retains_identity`, `invalid_pidfd_events_fail_closed`, `shutdown_registry_rejects_late_spawn` |
| INT-0008 | AC-6 | T-11903 / E07 | `source_driven_ci_contract` plus native Linux lifecycle CI job |
| INT-0008 | AC-6 | T-11903 / E08 | `sprint_phase_and_remote_audit` (independent review + recorded Git/GitHub/helper checks) |

## Unit Tests

### T-11901 shared boundary

- Intent: INT-0008 AC-6; E01/E02/E03.
- `scope_cleanup_success_timeout_unwind`: source-defined child modes exercise
  success, timeout, and unwind; cleanup outcome is asserted, not inferred.
- `leader_exit_reaps_descendants`: leader exits with a remaining controlled
  descendant holding both inherited stdout/stderr capture handles; exact process
  observation and group/Job accounting prove cleanup, and collection completes
  within the execution plus cleanup bounds (E01 and E03).
- `windows_spawn_failure_rolls_back`: deterministic post-create failure seam
  exercises assignment/resume rollback with an exact retained handle.
- `bounded_capture_head_tail_and_noisy_child`: oversized output, chosen prefix
  and suffix, both streams, and bounded completion without a pipe deadlock.
- Existing benchmark timeout/error/verification tests remain regression gates.

### T-11902 test ownership

- Intent: INT-0008 AC-6; E05/E06.
- `parent_watch_retains_identity`: source-defined live-parent/owner-death phases
  retain the actual owned descriptor across thread creation.
- `invalid_pidfd_events_fail_closed`: native event decoder rejects invalid/error
  descriptors and distinguishes exit from reaping; Linux-only where applicable.
- `shutdown_registry_rejects_late_spawn`: deterministic serialized registry
  transition refuses registrations after shutdown. A second interleaving races
  normal final absence/removal against shutdown signalling; an injected signal
  recorder proves removed registrations are never signalled from a stale
  snapshot, independently of the late-registration assertion.

## Integration Tests

- Intent: INT-0008 AC-6 and enabling AC-9; E04/E05.
- `cargo test -p ferric-process --locked` exercises the shared implementation.
- `cargo test -p ferric-bench --locked` preserves runner and verification semantics.
- `cargo test -p ferric-cli --locked` covers CLI units, `cli`, `bench_mock`,
  template hygiene, source containment, and registration concurrency.
- `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1`
  covers intentional managed-server detachment, token/watchdog ownership,
  exact owner death and reaping, and positive/negative lifecycle behavior.
- Windows local execution is required. Linux source execution is required in
  CI; the lifecycle suite retains its isolated non-root namespace. Tests that
  need reaping cannot accept a non-reaping host as success or use manual cleanup.

## End-to-End Tests

- **Status:** possible for the affected model-free command and lifecycle
  surfaces; the above CLI/bench/lifecycle suites are the E04/E05 E2E gate.
- `source_driven_ci_contract` (E07): source assertion checks no test-executable
  extraction/direct target launch and preserves Cargo, namespace, non-root,
  reaping, cancellation, and exit-code handling. Native Linux CI verifies it.
- `sprint_phase_and_remote_audit` (E08): independently review all five phase
  artifacts and clause coverage; confirm push SHA, `origin/main..dev` sprint
  scope, PR head/base/count, and required CI success. The owner performs merge.
- The real-model application E2E remains unlocked by INT-0007 T-11505/T-11506
  and T-11410/T-11412; this model-free refactor does not claim that result.

## Repository Gates and Evidence Rules

Run `cargo fmt --check`, workspace clippy with warnings denied, workspace tests,
CLI backend-openai clippy, lifecycle-feature clippy/test, and CI aarch64 checks.
Invoke source through Cargo only. Each failed attempt remains recorded;
correct and rerun affected gates after coherent changes. No old matrix is
acceptance evidence for the new dirty carryover. Record exact test names if a
logical planned test is implemented as multiple named cases. Test critique
and the owner's requested extra post-Loop adversarial audit must both be clean
before creating the one Sprint 119 PR.
