Finalized - DO NOT EDIT

# Sprint 116 Test Plan

## Intent Traceability

| Intent | Acceptance criterion | Build task / EARS clause | Verification |
| --- | --- | --- | --- |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-3 explicit stale/partial state | T-11601 / E01-A | `registration_inventory_retains_both_scopes_and_raw_bytes` |
| INT-0008 | AC-6 exact ownership | T-11601 / E01-B | `runfile_schema_authority_matrix` |
| INT-0008 | AC-3 concurrent invocation | T-11601 / E01-C | `concurrent_lifecycle_operations_are_per_path_safe` |
| INT-0008 | AC-3, AC-7 scoped cleanup | T-11601 / E01-D | `atomic_conditional_removal_matrix` |
| INT-0008 | AC-6 exact process ownership | T-11602 / E02-A | `retained_process_handle_identity_matrix` |
| INT-0008 | AC-6 listener ownership | T-11602 / E02-B | `loopback_listener_ownership_matrix` |
| INT-0008 | AC-6 spawned-child ownership | T-11602 / E02-C | `spawned_child_binding_window_matrix` |
| INT-0008 | AC-3 stale-state recovery | T-11603 / E03-A | `registration_resolution_cross_workspace_matrix` |
| INT-0008 | AC-4 truthful status | T-11603 / E03-B | `status_reports_scope_identity_health_and_next_action` |
| INT-0008 | AC-3, AC-4 shared state | T-11603 / E03-C | `registration_consumers_propagate_typed_ambiguity` |
| INT-0008 | AC-6, AC-7 owned teardown | T-11604 / E04-A | `down_signals_only_the_retained_handle` |
| INT-0008 | AC-4, AC-7 truthful terminal state | T-11604 / E04-B | `down_exit_and_listener_postconditions_gate_success` |
| INT-0008 | AC-3, AC-7 exact cleanup | T-11604 / E04-C | `down_cleanup_outcome_matrix` |
| INT-0008 | AC-6, AC-7 fail-closed cleanup | T-11604 / E04-D | `ambiguous_or_unverifiable_down_is_non_mutating` |
| INT-0008 | AC-3, AC-4 safe legacy recovery | T-11604 / E04-E | `live_v1_guidance_and_explicit_adoption` |
| INT-0008 | AC-3, AC-6 complete publication | T-11605 / E05-A | `registration_publication_is_complete_synced_and_no_clobber` |
| INT-0008 | AC-3, AC-6 partial-state recovery | T-11605 / E05-B | `partial_publication_stops_child_and_compensates_exactly` |
| INT-0008 | AC-7 and enabling AC-9 evidence | T-11605 / E05-C | `model_free_server_lifecycle_fixture_e2e` |
| INT-0008 | AC-6, AC-7 bounded external state | T-11605 / E05-D | `tailscale_mode_refuses_before_side_effects` |

The sprint advances but does not complete these intent criteria. INT-0008
AC-1/2's compact front door and AC-8/9's complete three-platform workflow and
usability matrix remain outside T-11504.

## Unit Tests

### T-11601 registration and concurrency tests

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `registration_inventory_retains_both_scopes_and_raw_bytes` (E01-A): local
  and global fixtures independently cover absent, unreadable, truncated JSON,
  symlink/non-regular state, valid v1, and valid v2; no slot shadows another
  and every captured slot retains its original path and exact bytes. A valid
  global v2 origin is captured separately, including changed/missing/blocked
  origin state.
- `runfile_schema_authority_matrix` (E01-B): v2 round-trips PID, tagged start
  token, executable, and argv; v1 parses as legacy/non-authorizing; zero PID or
  port, empty start token/executable/argv or argv elements, a missing identity
  field,
  relative executable, missing/relative/wrong-suffix originating local path,
  local self-mismatch, non-Tailscale base URL wrong scheme/host/port/path, and
  unknown future versions fail closed. A global capture retains the absolute
  originating-local coordinate for cross-workspace alias cleanup; a
  `tailscale: true` record remains blocked before its remote base URL is used.
- `identical_and_parse_equal_mirrors_keep_scope_tokens` (E01-A, E01-B):
  byte-identical and semantically identical/differently formatted mirrors can
  group to one key while keeping separate raw-byte cleanup tokens.
- `concurrent_lifecycle_operations_are_per_path_safe` (E01-C): scripted
  interleavings from two clients in different workspaces and one configured
  global root prove final-name no-clobber, typed split observations, exact
  attempt-owned compensation, and no cross-scope linearizability claim.
- `atomic_conditional_removal_matrix` (E01-D): unchanged bytes delete after
  same-parent isolation; changed
  raw-but-parse-equivalent bytes, replacements, unreadable files, and removal
  errors remain. A concurrent writer may recreate the original name after
  isolation, but that replacement is never opened or removed. Restoration
  covers success, original-name `AlreadyExists`
  (preserve both), and every other injected I/O error (failure plus exact
  reported holding path).

### T-11602 platform identity tests

- **Intent:** INT-0008
- `retained_process_handle_identity_matrix` (E02-A): mutate start token, Linux
  boot identity, executable, argv, and PID one coordinate at a time; authority
  requires the same retained object and signal/wait APIs never accept a PID.
- `pid_reuse_before_handle_acquisition_signals_nothing` (E02-A): a different
  process at the recorded number remains untouched.
- `pid_reuse_after_handle_acquisition_targets_original_handle` (E02-A): after
  the fake numeric mapping changes, termination still addresses only the
  originally retained object.
- `loopback_listener_ownership_matrix` (E02-B): exact registered IPv4 loopback,
  absent listener, wildcard/public bind, wrong port/state, foreign inode/PID,
  multiple owners, and inspection failure yield their exact typed states.
  Windows authority is limited to the registered `127.0.0.1` endpoint; Linux
  covers exact IPv4 plus `/proc/net/tcp6` wildcard/dual-stack ambiguity and
  does not mistake it for exclusive ownership.
- `spawned_child_binding_window_matrix` (E02-C): Windows acquires identity from
  the spawned `Child` process object rather than reopening its PID; Linux opens
  a pidfd immediately and checks the original child is still running. Injected
  exit/PID reuse before binding, during readiness, before publication, and on
  every launch-failure cleanup proves zero signal to the replacement and all
  terminate/wait calls target only the retained generation.

### T-11603 resolution and status tests

- **Intent:** INT-0008
- `registration_resolution_cross_workspace_matrix` (E03-A): exact aliases,
  global-only discovery, stale-local A plus live-global B, the symmetric case,
  two dead records, one live plus one stale, two live keys, malformed peer,
  unreadable peer, live legacy state, and present/absent-PID `tailscale: true`
  state all produce typed outcomes without scope precedence or premature PID
  inspection. Same process token with one-at-a-time engine/port/base-url/origin
  metadata mismatch blocks. A stale record's absent listener permits cleanup;
  a listener owned by the selected same-port target is reconciled, while a
  foreign/uninspectable/wrong-port owner blocks every stale deletion.
- `status_reports_scope_identity_health_and_next_action` (E03-B): healthy,
  unhealthy, stale, conflict, unverifiable, and live-v1 states list both scopes
  and exact next action; HTTP 200 never overrides process/listener mismatch.
- `registration_consumers_propagate_typed_ambiguity` (E03-C): backend,
  autonomy, doctor, status, and down consume the same typed result and never
  reinterpret conflict or inspection failure as no runfile/default endpoint.

### T-11604 teardown and adoption tests

- **Intent:** INT-0008
- `down_signals_only_the_retained_handle` (E04-A): one exact target records one
  handle-targeted signal and zero PID-targeted signals; healthy/unhealthy HTTP
  does not alter authority, and an absent listener permits hung-server stop.
- `down_exit_and_listener_postconditions_gate_success` (E04-B): signal error,
  wait timeout, non-exited handle, and retained target listener each prevent a
  success claim and every registration deletion.
- `down_cleanup_outcome_matrix` (E04-C): stopped aliases, dead/reused stale
  records, already-absent paths, changed bytes, unreadable replacements, and
  removal/restore failures render distinct outcomes and preserve/report every
  holding path; stale cleanup never says stopped and never runs while stale
  listener ownership is unreconciled.
- `ambiguous_or_unverifiable_down_is_non_mutating` (E04-D): two live keys,
  malformed/unreadable peer state, live v1, and foreign/multiple/uninspectable
  listener state record zero signal/delete calls and no stopped wording.
- `live_v1_guidance_and_explicit_adoption` (E04-E): diagnostics retain and name
  every v1 alias and round-trip the exact `server adopt --pid` argv; adoption
  changes no process, requires explicit matching PID plus retained-handle
  engine/available-argv/listener proof, conditionally upgrades only unchanged
  aliases, and rejects mismatch or insufficient evidence without mutation.

### T-11605 publication tests

- **Intent:** INT-0008
- `registration_publication_is_complete_synced_and_no_clobber` (E05-A): one
  serialization yields byte-identical parseable finals; short writes remain
  only in exclusive stages; file sync precedes no-clobber persist; destination
  appearance causes no replacement; directory sync is attempted where
  supported; persist/durability failure is launch failure, not final write;
  stage cleanup succeeds on every pre-commit failure or its preserved path is
  reported as part of the failure.
- `partial_publication_stops_child_and_compensates_exactly` (E05-B): every
  first/second stage, persist, directory-sync, child-exit, and rollback boundary
  stops/waits the held child. Injected terminate error, wait error, and wait
  timeout prove no published registration is rolled back without retained
  generation exit; after proven exit, only unchanged attempt-owned finals and
  stages are removed. External replacements survive, rollback/stage cleanup
  failures and their paths are reported, and every branch returns failure.
  Global `None` covers the legitimate local-only branch.

## Integration Tests

### Registration and lifecycle composition

- **Intents:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `two_process_lifecycle_interleaving_is_per_path_safe` (E01-C, E01-D): two
  real child processes target different workspaces and one isolated global
  root; injected interleavings prove one final path is never clobbered, a
  partial loser is compensated exactly, intermediate split state stays typed,
  and no unrelated entry is removed.
- `status_and_discovery_two_scope_matrix` (E03-A through E03-C): temp
  workspaces A/B and child-scoped global state prove both scopes are listed,
  stale local does not shadow one live global, and ambiguity never falls back.
- `down_retained_handle_transition_matrix` (E02-A, E02-B, E04-A through
  E04-D): a scripted runtime moves through PID reuse, listener transfer,
  unhealthy endpoint, exit, wait timeout, and post-capture mutation while
  asserting exact acquire/signal/wait/store order. Stale listener ownership is
  cleaned only when absent or accounted for by the selected same-port target.
- `up_spawned_child_binding_precedes_readiness` (E02-C, E05-A): a scripted
  child/adapter asserts Windows Child-handle duplication or Linux pidfd binding
  occurs immediately after spawn and before the first health poll; original
  child exit plus numeric reuse cannot redirect cleanup or publication.
- `legacy_adoption_then_down` (E04-E, E04-A): an isolated harmless live v1
  process cannot be stopped before explicit adoption, upgrades to v2 without a
  signal, and only a later generation-matching down terminates it.
- `registration_publication_failure_matrix` (E05-A, E05-B): injected stage,
  sync, no-clobber persist, second-scope, compensation, and child-exit failures
  prove rollback follows proven retained-child exit and no partially registered
  live process loses its recovery registration.
- `tailscale_mode_refuses_before_side_effects` (E05-D): a fake Tailscale
  executable, fake engine, empty registration roots, and sentinels prove the
  request fails before either executable runs and creates no file, process,
  listener, coordination artifact, stage, or reset attempt. Doctor reports
  BLOCKED. A `tailscale: true` fixture blocks status/down cleanup before PID
  inspection for both present and absent PIDs and preserves every record.

### Platform adapters

- `windows_retained_process_handle_smoke` (E02-A, E02-B, E02-C, E04-A, E04-B): on
  Windows, spawn a harmless owned child, acquire one real process-object
  handle, re-read exact creation/executable identity, classify listener state,
  terminate/wait through the handle, and close it without `taskkill`.
- `linux_pidfd_process_handle_smoke` (E02-A, E02-B, E02-C, E04-A, E04-B): on Linux,
  spawn a harmless owned child, capture boot/start identity, acquire a pidfd,
  classify listener state, signal/poll through that descriptor, and prove a
  later numeric mapping is irrelevant.
- These adapter tests use no model, engine, user registration, or fixed port.

## End-to-End Tests

- **Status:** possible
- `cross_workspace_stale_local_live_global_lifecycle` (E01-A, E01-C, E02-A,
  E02-B, E03-A, E03-B, E04-A through E04-D): on Windows and Linux, isolate the
  global root, create workspace A's stale local registration and one harmless
  verified global process B, run real CLI status/down from A, and pass only
  when B's retained handle is stopped, A stale cleanup is separate, both B's
  originating local and global aliases disappear, and unrelated sentinels
  remain.
- `model_free_server_lifecycle_fixture_e2e` (E05-C): build the feature-gated
  Rust fixture binary, copy it into a temp bin directory under the platform's
  exact `llama-server` filename, prepend only that directory to Ferric's child
  `PATH`, and supply a dummy regular model. The fixture must parse ordinary
  closed-engine argv and serve loopback health. Real CLI up/status/down must
  leave no fixture process, listener, final/staged registration, coordination
  artifact, or unrelated mutation on both Windows and Linux.
- `tailscale_refusal_has_zero_external_effects` (E05-D): the real CLI receives
  `server up --tailscale` with isolated fake engine/Tailscale binaries and
  exits non-success before invoking either one; doctor says BLOCKED. Separate
  stale/live `tailscale: true` fixtures prove status/down inspect no PID, signal
  nothing, delete nothing, and retain the durable-state clue. Invocation logs,
  registration roots, process table, and sentinels remain unchanged.

The complete installed `run/status/resume/explain/evidence/cleanup` usability
E2E remains unlocked by [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
through ordered task T-11509. Sprint 116 proves only the lifecycle primitive
that workflow will compose.
