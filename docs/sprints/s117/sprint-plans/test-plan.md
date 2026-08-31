Finalized - DO NOT EDIT

# Sprint 117 Test Plan

## Intent Traceability

| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|---|---|---|---|
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-3 explicit partial/stale state | T-11701 / E01-A | `registration_inventory_retains_both_scopes_and_raw_bytes` |
| INT-0008 | AC-6 exact registration authority | T-11701 / E01-B | `runfile_schema_authority_matrix` |
| INT-0008 | AC-3 concurrent invocation | T-11701 / E01-C | `concurrent_lifecycle_operations_are_per_path_safe` |
| INT-0008 | AC-3, AC-7 bounded cleanup | T-11701 / E01-D | `atomic_conditional_removal_matrix` |
| INT-0008 | AC-6 exact retained process ownership | T-11702 / E02-A | `retained_process_handle_identity_matrix` |
| INT-0008 | AC-6 exact listener ownership | T-11702 / E02-B | `loopback_listener_ownership_matrix`; `up_nonexclusive_listener_stops_retained_child_and_publishes_nothing` |
| INT-0008 | AC-6 spawned-child ownership | T-11702 / E02-C | `spawned_child_binding_window_matrix` |
| INT-0008 | AC-3, AC-6 typed ownership resolution | T-11703 / E03-A | `registration_resolution_cross_workspace_matrix` |
| INT-0008 | AC-4 truthful status and next action | T-11703 / E03-B | `status_reports_scope_identity_health_and_next_action` |
| INT-0008 | AC-3, AC-4, AC-6 shared consumer policy | T-11703 / E03-C | `registration_consumers_propagate_typed_ambiguity` |
| INT-0008 | AC-6, AC-7 owned teardown | T-11704 / E04-A | `down_signals_only_the_retained_handle` |
| INT-0008 | AC-4, AC-7 truthful terminal state | T-11704 / E04-B | `down_exit_and_listener_postconditions_gate_success` |
| INT-0008 | AC-3, AC-7 exact cleanup | T-11704 / E04-C | `down_cleanup_outcome_matrix` |
| INT-0008 | AC-6, AC-7 fail-closed mutation | T-11704 / E04-D | `ambiguous_or_unverifiable_down_is_non_mutating` |
| INT-0008 | AC-3, AC-4 safe legacy recovery | T-11704 / E04-E | `live_v1_guidance_and_explicit_adoption` |
| INT-0008 | AC-3, AC-6 complete publication | T-11705 / E05-A | `registration_publication_is_complete_synced_and_no_clobber` |
| INT-0008 | AC-3, AC-6 exact compensation | T-11705 / E05-B | `partial_publication_stops_child_and_compensates_exactly` |
| INT-0008 | AC-7 bounded external state | T-11705 / E05-D | `tailscale_mode_refuses_before_side_effects` |
| INT-0008 | AC-7 and enabling AC-9 evidence | T-11706 / E05-C | `model_free_server_lifecycle_fixture_e2e` |

The first verification name in each row is one of the exact nineteen frozen
acceptance names and forms the acceptance API; later names in a row are
supplemental regressions for additional elementary triggers. Renaming a frozen
name requires updating both locked plans before finalization; aggregate test
counts or a differently named neighbor do not satisfy its row.

## Unit Tests

### T-11701 registration authority and atomic-store tests

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md), AC-3/6/7; clauses E01-A through E01-D.
- `registration_inventory_retains_both_scopes_and_raw_bytes` (E01-A):
  table-drive configured/unconfigured local/global slots as absent, unreadable,
  malformed, nonregular, v1, and v2; independently observe matching, absent,
  changed, malformed, and unreadable promised origins. Assert typed scope,
  exact path, source origin, raw bytes, and parsed state survive without
  fallback or reserialization.
- `runfile_schema_authority_matrix` (E01-B): accept canonical current-platform
  v2 and readable v1; reject arbitrary/empty/foreign/noncanonical tokens,
  overflow/zero coordinates, relative/empty identity, empty argv elements,
  missing/relative/wrong-suffix/self-mismatched origin, incorrect base URL, and
  unknown versions. Rows assert `InvalidSchema`, not stale cleanup.
- `runfile_schema_rejects_untagged_foreign_or_noncanonical_start_tokens`
  (E01-B): focused regression for `token`, `opaque`, wrong tag, leading zero,
  malformed UUID/field order, zero start ticks, and trailing data.
- `identical_and_parse_equal_mirrors_keep_scope_tokens` (E01-A/E01-D):
  byte-identical and formatting-different parse-equal mirrors retain distinct
  exact-byte cleanup captures while resolving metadata equivalence correctly.
- `concurrent_lifecycle_operations_are_per_path_safe` (E01-C): scripted store
  barriers cover inventory during each publication/removal stage,
  winner/loser no-clobber publication, a typed split view, two concurrent
  conditional adoption replacements, attempt-owned compensation,
  remove-versus-publish, and two removers without claiming a cross-scope
  transaction.
- `atomic_conditional_removal_matrix` (E01-D): exact, absent,
  raw-different-but-parse-equal, nonregular/unreadable, replacement after move,
  moved-entry read/remove failure, restore success, occupied original, and
  other restore failures. Assert original/holding bytes and typed outcomes.

### T-11702 retained-process and listener tests

- **Intent:** INT-0008 AC-6; clauses E02-A through E02-C.
- `retained_process_handle_identity_matrix` (E02-A): a fake PID map changes
  generations before and after acquisition; start token, executable, and argv
  mismatches fail closed; signal/wait ledger contains only the opaque retained
  generation.
- `pid_reuse_before_handle_acquisition_signals_nothing` (E02-A/E02-C): binding
  sees the replacement and records no signal.
- `pid_reuse_after_handle_acquisition_targets_original_handle` (E02-A): a
  remapped numeric PID does not change the recorded terminate/wait generation.
- `linux_non_utf8_argv_is_uninspectable_not_lossy` (E02-A, Linux): invalid
  `/proc/<pid>/cmdline` bytes produce inspection failure, never a replacement
  character in authority.
- `loopback_listener_ownership_matrix` (E02-B): exact IPv4 loopback, absent,
  wildcard IPv4, IPv6 wildcard/dual-stack, foreign, multiple, shared, and
  uninspectable states, plus Windows exact `::1` as explicitly unsupported;
  only exclusive target IPv4 loopback or absent is authorizing.
- `linux_uninspectable_shared_listener_owner_is_not_exclusive` (E02-B,
  Linux): an unreadable peer sharing the target inode cannot produce an
  exclusive result; record any unavoidable kernel visibility limitation.
- `wildcard_listener_blocks_teardown_and_preserves_registration` (E02-B/E04-D):
  replaces the prior success expectation and asserts an empty signal/delete
  ledger.
- `up_nonexclusive_listener_stops_retained_child_and_publishes_nothing`
  (E02-B/E02-C/E05-A): after retained binding and readiness, wildcard/public,
  foreign, multiple, shared, and uninspectable ownership each block
  publication; exact child terminate/wait/reap is proved and no runfile exists,
  or the failure report names preserved recovery state.
- `spawned_child_binding_window_matrix` (E02-C): exit/reuse before bind, bind
  failure, error immediately after bind, exit during readiness, and healthy
  bound child; every cleanup targets the retained generation or preserves a
  recovery clue.
- `bound_child_try_wait_error_uses_retained_cleanup_or_preserves_recovery`
  (E02-C/E05-B): focused orphan regression.

### T-11703 typed discovery, status, and consumer tests

- **Intent:** INT-0008 AC-3/4/6; clauses E03-A through E03-C.
- `registration_resolution_cross_workspace_matrix` (E03-A): exact aliases,
  global-only, both stale/live directions, stale-only, two live keys,
  same-token field-by-field metadata conflict, malformed/unreadable peer, live
  v1, missing/changed origin, Tailscale present/absent PID with zero process
  calls, and all stale-listener reconciliation outcomes. Assert typed
  Empty/Ready/Conflict/Unverifiable variants and no scope precedence.
- `status_reports_scope_identity_health_and_next_action` (E03-B): assert pure
  report and rendered stdout/stderr for empty, healthy, unhealthy, absent
  listener, stale, split, conflict, unverifiable, wildcard, missing origin, and
  live-v1 cases. Every case lists configured local/global/origin state,
  identity/listener/health, and one exact safe action.
- `registration_consumers_propagate_typed_ambiguity` (E03-C): feed identical
  Empty/Ready/Degraded/StaleOnly/Conflict/Unverifiable fixtures to backend,
  strict autonomy, doctor, status, and down. Automatic Empty alone may select
  the default; explicit selection remains explicitly typed; all blocked states
  stay blocked without mutation or HTTP/process/binary probes.
- `strict_autonomy_requires_fresh_managed_discovery_before_http` (E03-C): no
  HTTP call precedes a Ready result, and final revalidation rejects a newly
  introduced conflict, changed registration key, or missing alias.
- `doctor_blocks_before_external_probes` (E03-B/E03-C/E05-D): registration or
  Tailscale blockers produce a report before engine version/model/PID effects.

### T-11704 teardown, cleanup, and adoption tests

- **Intent:** INT-0008 AC-3/4/6/7; clauses E04-A through E04-E.
- `down_signals_only_the_retained_handle` (E04-A): exact target with owned or
  absent listener and healthy/unhealthy HTTP records one retained-generation
  terminate/wait and no PID-based call.
- `down_exit_and_listener_postconditions_gate_success` (E04-B): signal error,
  wait timeout/error, post-exit target/wildcard/foreign/uninspectable listener,
  successful release, and already-exited rows. No failure row deletes a
  registration or renders `stopped`; a proven-exited child is always reaped.
- `down_cleanup_outcome_matrix` (E04-C): stopped, stale-cleaned,
  already-absent, replacement-preserved, restore-failed, removal-failed, and
  partial multi-alias outcomes; assert exact per-path ordering and final
  report, including every preserved holding path.
- `ambiguous_or_unverifiable_down_is_non_mutating` (E04-D): two live keys,
  malformed/unreadable peer, live v1, wildcard/shared/foreign/uninspectable
  listener, invalid token, and durable Tailscale; assert empty acquire where
  required, empty signal/delete ledgers, byte-identical records, and no
  `stopped` output.
- `malformed_v2_token_blocks_down_without_signal_or_delete` (E01-B/E04-D):
  focused schema-to-mutation regression.
- `live_v1_guidance_and_explicit_adoption` (E04-E): status and down retain all
  aliases and render the complete `ferric server adopt --pid <pid>` command;
  successful adoption checks closed executable, every present argv coordinate,
  exact listener, unchanged aliases, final generation recheck, and never
  signals.
- `legacy_adoption_transition_and_rollback_matrix` (E04-E/E04-C): executable,
  argv, listener, identity-transition, replacement, rollback, and partial
  failure rows preserve exact recovery bytes and report failed coordinates.

### T-11705 publication, compensation, and Tailscale tests

- **Intent:** INT-0008 AC-3/6/7; clauses E05-A, E05-B, and E05-D.
- `registration_publication_is_complete_synced_and_no_clobber` (E05-A): one
  serialization; local-only and mirrored success; byte identity/parseability;
  same-parent exclusive stage; write, flush, file sync, atomic no-replace, and
  Unix parent sync ordering; local/global precommit and committed durability
  failures; existing final and alias paths; zero unexplained stage residue.
- `partial_publication_stops_child_and_compensates_exactly` (E05-B): local or
  global partial commit, child exit during publication, terminate failure,
  wait timeout/error, listener survival, successful exit, unchanged rollback,
  concurrent replacement, conditional cleanup failure, and stage cleanup
  failure. Assert child-stop/wait precedes rollback and every unproved-exit row
  retains all published recovery captures.
- `doctor_tailscale_block_precedes_binary_model_and_network_probes` (E05-D):
  the pure doctor policy reports blocked before any external probe.
- `tailscale_registration_blocks_before_process_inspection` (E03-A/E05-D):
  present- and absent-PID records produce zero acquire/inspect/signal/remove
  events and preserve exact bytes.
- `tailscale_blocked_commands_preserve_records_and_never_reset` (E05-D):
  rendered status/down guidance explains scoped proxy cleanup is unavailable
  and contains no blind node-wide reset instruction.

## Integration Tests

### Registration and resolver composition

- **Intents:** INT-0008 AC-3/4/6/7; E01-C/D and E03-A/B/C.
- `two_process_lifecycle_interleaving_is_per_path_safe`: distinct child test
  clients coordinate through filesystem barriers for two workspaces racing one
  global publication, inventory at every exposed intermediate state,
  publisher-versus-remover replacement, two explicit adopters racing one live
  v1 record, and two removers. Require typed split inventory, one global/adoption
  winner, loser compensation only of unchanged attempt-owned bytes,
  preservation of the concurrent replacement, no signal during adoption, and
  one Removed plus one Absent.
- `status_and_discovery_two_scope_matrix`: compose the production store with
  scripted process/listener/health facts and send one typed inventory through
  status, backend, strict autonomy, doctor, and down.

### Process and lifecycle composition

- **Intents:** INT-0008 AC-3/4/6/7; E02-A/B/C and E04-A/B/C/D/E.
- `down_retained_handle_transition_matrix`: scripted Observe → PreSignal →
  PostExit → PreCleanup facts include PID remap, listener transfer, failures,
  and per-alias outcomes; assert event order and final report.
- `up_spawned_child_binding_precedes_readiness`: scripted spawn/bind/readiness/
  publication transitions prove no readiness or publication occurs before
  retained binding.
- `legacy_adoption_then_down`: adoption publishes a canonical current-OS token
  without signal; subsequent down reacquires exactly that generation and
  cleans unchanged aliases after exit.
- `registration_publication_failure_matrix`: production publisher plus scripted
  persistence/runtime/store adapters covers every E05-A/B fault boundary.

### Native platform smokes

- `windows_retained_process_handle_smoke` (Windows; E02-A/B/C, E04-A/B): a
  harmless child yields a canonical FILETIME token, exact loopback/wildcard
  classification, explicit non-authorizing exact-`::1` observation,
  handle-only terminate/wait, and handle release.
- `linux_pidfd_process_handle_smoke` (Linux x86_64/AArch64 little-endian;
  E02-A/B/C, E04-A/B): a harmless child yields boot-id/start-ticks token,
  `/proc` identity, IPv4/IPv6 classification, pidfd-only signal/poll, and fd
  release. Include a visible inherited-fd shared-owner row.

## End-to-End Tests

- **Status:** possible.
- `model_free_server_lifecycle_fixture_e2e` (E05-C): the feature-gated Rust
  fixture copied to the closed engine filename accepts ordinary argv and a
  dummy regular model, then the real CLI performs up/status/down and
  stale-local/live-global recovery across isolated workspaces. A guard exists
  before each blocking call; final assertions prove no process, listener,
  registration, stage, coordination artifact, or sentinel mutation remains.
- `legacy_adoption_then_down_cli_e2e` (E04-E/E04-A): a harmless live v1
  fixture first makes status/down print exact adoption guidance without signal,
  then real adopt and down complete safely.
- `tailscale_mode_refuses_before_side_effects` (E05-D): a fake engine and fake
  Tailscale executable both carry invocation markers. Real up/doctor/status/down
  cases leave markers absent, bytes/process/listener/sentinels unchanged, and
  render the complete blocked explanation.

The fixture suite uses a process-wide lock, per-child lifetime token, bounded
watchdog, independently handled HTTP connections, and only a diagnosed
address-in-use retry. It runs with `--test-threads=1` as a second guard.

## Verification Commands

1. Presence and routing:

   ```text
   cargo test -p ferric-cli --all-features -- --list
   cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture -- --list
   ```

2. Clause matrices: execute each of the nineteen traceability names with
   `cargo test -p ferric-cli --all-features <name> -- --nocapture`; where a
   name is platform-gated, record the native CI command and runner instead of
   claiming a local pass.

3. Focused package gates:

   ```text
   cargo fmt --check
   cargo clippy -p ferric-cli --all-targets --all-features -- -D warnings
   cargo test -p ferric-cli --all-features --locked
   cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1
   ```

4. Repository gates:

   ```text
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features --locked
   rustup target add aarch64-unknown-linux-gnu
   cargo check -p ferric-cli --features lifecycle-fixture --all-targets --target aarch64-unknown-linux-gnu --locked
   ```

5. CI gate: a dedicated Ubuntu/Windows matrix runs fixture clippy plus the
   serialized feature test with a 12-minute job timeout. Record immutable run
   URLs and heads. AArch64 compile-check evidence is separate from native
   x86_64/AArch64 runtime claims.

## Test Evidence Contract

- `unit-tests.md`, `integration-tests.md`, and `e2e-tests.md` record exact
  commands, exit status, target/runner, immutable implementation head, and the
  clause names proved.
- `test-report.md` contains a nineteen-row result ledger; each row links its
  command evidence and may be only pass or fail/unproved.
- The independent Test critic reads the locked plans, implementation diff, and
  all Test artifacts. T-11606 is completed only for `clean` or fully addressed
  `proceed-with-caveats`; any missing name, command, platform result, immutable
  head, or CI gate fails Sprint 117 closed.
