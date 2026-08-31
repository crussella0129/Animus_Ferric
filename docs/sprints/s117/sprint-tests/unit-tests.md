# Sprint 117 Unit and Static Test Evidence

**Intent:** [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md),
AC-3/4/6/7 and enabling evidence toward AC-9.

## Evidence identity

- **Immutable implementation head:**
  `b679a25ba83069ab849b0f7f2eb8a3269eba10c5` (`dev`).
- **Local runner:** Microsoft Windows NT 10.0.26200.0, x64,
  `rustc 1.96.0 (ac68faa20 2026-05-25)`,
  `x86_64-pc-windows-msvc`.
- **Locked plan objects:** build plan
  [`5f1240c2e65a8e78f5376d86f50f4602d55a790c`](../sprint-plans/build-plan.md);
  test plan
  [`e9486068fa471eb018d3e344f88b2c7fad38e009`](../sprint-plans/test-plan.md).
- **Protected unrelated artifact:**
  `docs/sprints/s114/control-artifacts/model/acquisition-tests.json` remained
  unstaged with SHA-256
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`.

Every command below ran from the repository root. Unless a row says
otherwise, the target was the local Windows runner, the exit status was 0,
and exactly one intended test passed.

## Presence and routing

| Command | Result |
|---|---|
| `cargo test -p ferric-cli --all-features -- --list` | Exit 0; enumerated 255 binary unit tests, 6 benchmark integrations, 69 CLI integrations, 3 lifecycle fixtures, and 3 template-hygiene tests. All nineteen frozen names were present. |
| `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture -- --list` | Exit 0; enumerated exactly `legacy_adoption_then_down_cli_e2e`, `model_free_server_lifecycle_fixture_e2e`, and `tailscale_mode_refuses_before_side_effects`. |

## Frozen clause matrices

The two frozen fixture rows, E05-C and E05-D, are recorded in
[end-to-end evidence](e2e-tests.md#frozen-end-to-end-rows). The other seventeen
frozen acceptance commands were:

| Clause | Exact acceptance name | Exact command | Result |
|---|---|---|---|
| E01-A | `registration_inventory_retains_both_scopes_and_raw_bytes` | `cargo test -p ferric-cli --all-features registration_inventory_retains_both_scopes_and_raw_bytes -- --nocapture` | Exit 0; exactly 1 passed. |
| E01-B | `runfile_schema_authority_matrix` | `cargo test -p ferric-cli --all-features runfile_schema_authority_matrix -- --nocapture` | Exit 0; exactly 1 passed. |
| E01-C | `concurrent_lifecycle_operations_are_per_path_safe` | `cargo test -p ferric-cli --all-features concurrent_lifecycle_operations_are_per_path_safe -- --nocapture` | Exit 0; exactly 1 passed. |
| E01-D | `atomic_conditional_removal_matrix` | `cargo test -p ferric-cli --all-features atomic_conditional_removal_matrix -- --nocapture` | Exit 0; exactly 1 passed. |
| E02-A | `retained_process_handle_identity_matrix` | `cargo test -p ferric-cli --all-features retained_process_handle_identity_matrix -- --nocapture` | Exit 0; exactly 1 passed. |
| E02-B | `loopback_listener_ownership_matrix` | `cargo test -p ferric-cli --all-features loopback_listener_ownership_matrix -- --nocapture` | Exit 0; exactly 1 passed. |
| E02-C | `spawned_child_binding_window_matrix` | `cargo test -p ferric-cli --all-features spawned_child_binding_window_matrix -- --nocapture` | Exit 0; exactly 1 passed. |
| E03-A | `registration_resolution_cross_workspace_matrix` | `cargo test -p ferric-cli --all-features registration_resolution_cross_workspace_matrix -- --nocapture` | Exit 0; exactly 1 passed. |
| E03-B | `status_reports_scope_identity_health_and_next_action` | `cargo test -p ferric-cli --all-features status_reports_scope_identity_health_and_next_action -- --nocapture` | Exit 0; exactly 1 passed. |
| E03-C | `registration_consumers_propagate_typed_ambiguity` | `cargo test -p ferric-cli --all-features registration_consumers_propagate_typed_ambiguity -- --nocapture` | Exit 0; exactly 1 passed. |
| E04-A | `down_signals_only_the_retained_handle` | `cargo test -p ferric-cli --all-features down_signals_only_the_retained_handle -- --nocapture` | Exit 0; exactly 1 passed. |
| E04-B | `down_exit_and_listener_postconditions_gate_success` | `cargo test -p ferric-cli --all-features down_exit_and_listener_postconditions_gate_success -- --nocapture` | Exit 0; exactly 1 passed. |
| E04-C | `down_cleanup_outcome_matrix` | `cargo test -p ferric-cli --all-features down_cleanup_outcome_matrix -- --nocapture` | Exit 0; exactly 1 passed. |
| E04-D | `ambiguous_or_unverifiable_down_is_non_mutating` | `cargo test -p ferric-cli --all-features ambiguous_or_unverifiable_down_is_non_mutating -- --nocapture` | Exit 0; exactly 1 passed. |
| E04-E | `live_v1_guidance_and_explicit_adoption` | `cargo test -p ferric-cli --all-features live_v1_guidance_and_explicit_adoption -- --nocapture` | Exit 0; exactly 1 passed. |
| E05-A | `registration_publication_is_complete_synced_and_no_clobber` | `cargo test -p ferric-cli --all-features registration_publication_is_complete_synced_and_no_clobber -- --nocapture` | Exit 0; exactly 1 passed. |
| E05-B | `partial_publication_stops_child_and_compensates_exactly` | `cargo test -p ferric-cli --all-features partial_publication_stops_child_and_compensates_exactly -- --nocapture` | Exit 0; exactly 1 passed. |

The filters produced no frozen zero-test or multiple-match result. Cargo also
reported zero tests in unrelated integration binaries after filtering; those
were expected harness routing, not missing acceptance tests.

## Supplemental unit regressions

All twenty-two Windows-applicable supplemental names in the locked plan were
run as individual filters and exited 0. Six composition rows, one native
Windows smoke, and one CLI E2E are detailed in the other evidence files. The
fourteen unit-level rows were:

| Clauses | Supplemental name | Exact command and result |
|---|---|---|
| E01-B | `runfile_schema_rejects_untagged_foreign_or_noncanonical_start_tokens` | `cargo test -p ferric-cli --all-features --locked runfile_schema_rejects_untagged_foreign_or_noncanonical_start_tokens -- --nocapture` — exit 0; 1 passed. |
| E01-A/D | `identical_and_parse_equal_mirrors_keep_scope_tokens` | `cargo test -p ferric-cli --all-features --locked identical_and_parse_equal_mirrors_keep_scope_tokens -- --nocapture` — exit 0; 1 passed. |
| E02-A/C | `pid_reuse_before_handle_acquisition_signals_nothing` | `cargo test -p ferric-cli --all-features --locked pid_reuse_before_handle_acquisition_signals_nothing -- --nocapture` — exit 0; 1 passed. |
| E02-A | `pid_reuse_after_handle_acquisition_targets_original_handle` | `cargo test -p ferric-cli --all-features --locked pid_reuse_after_handle_acquisition_targets_original_handle -- --nocapture` — exit 0; 1 passed. |
| E02-B/E04-D | `wildcard_listener_blocks_teardown_and_preserves_registration` | `cargo test -p ferric-cli --all-features --locked wildcard_listener_blocks_teardown_and_preserves_registration -- --nocapture` — exit 0; 1 passed. |
| E02-B/C, E05-A | `up_nonexclusive_listener_stops_retained_child_and_publishes_nothing` | `cargo test -p ferric-cli --all-features --locked up_nonexclusive_listener_stops_retained_child_and_publishes_nothing -- --nocapture` — exit 0; 1 passed. |
| E02-C/E05-B | `bound_child_try_wait_error_uses_retained_cleanup_or_preserves_recovery` | `cargo test -p ferric-cli --all-features --locked bound_child_try_wait_error_uses_retained_cleanup_or_preserves_recovery -- --nocapture` — exit 0; 1 passed. |
| E03-C | `strict_autonomy_requires_fresh_managed_discovery_before_http` | `cargo test -p ferric-cli --all-features --locked strict_autonomy_requires_fresh_managed_discovery_before_http -- --nocapture` — exit 0; 1 passed. |
| E03-B/C, E05-D | `doctor_blocks_before_external_probes` | `cargo test -p ferric-cli --all-features --locked doctor_blocks_before_external_probes -- --nocapture` — exit 0; 1 passed. |
| E01-B/E04-D | `malformed_v2_token_blocks_down_without_signal_or_delete` | `cargo test -p ferric-cli --all-features --locked malformed_v2_token_blocks_down_without_signal_or_delete -- --nocapture` — exit 0; 1 passed. |
| E04-E/C | `legacy_adoption_transition_and_rollback_matrix` | `cargo test -p ferric-cli --all-features --locked legacy_adoption_transition_and_rollback_matrix -- --nocapture` — exit 0; 1 passed. |
| E05-D | `doctor_tailscale_block_precedes_binary_model_and_network_probes` | `cargo test -p ferric-cli --all-features --locked doctor_tailscale_block_precedes_binary_model_and_network_probes -- --nocapture` — exit 0; 1 passed. |
| E03-A/E05-D | `tailscale_registration_blocks_before_process_inspection` | `cargo test -p ferric-cli --all-features --locked tailscale_registration_blocks_before_process_inspection -- --nocapture` — exit 0; 1 passed. |
| E05-D | `tailscale_blocked_commands_preserve_records_and_never_reset` | `cargo test -p ferric-cli --all-features --locked tailscale_blocked_commands_preserve_records_and_never_reset -- --nocapture` — exit 0; 1 passed. |

The short supplemental filter `legacy_adoption_then_down` initially matched
both the unit test and its CLI E2E substring neighbor. The unambiguous rerun

```text
cargo test -p ferric-cli --all-features --locked server::tests::legacy_adoption_then_down -- --exact --nocapture
```

exited 0 with exactly one pass.

Two Linux-only negative-path regressions were observed by exact name in the
successful ordinary Ubuntu workspace job
[99366993856](https://github.com/crussella0129/Animus_Ferric/actions/runs/33351978700/job/99366993856)
at the immutable head:

| Exact executed name | Result and clause |
|---|---|
| `server_process::platform::tests::linux_non_utf8_argv_is_uninspectable_not_lossy` | `ok` inside the 250/250 Linux `ferric` unit binary; E02-A preserves non-UTF-8 argv as uninspectable rather than lossy identity. |
| `server_process::platform::tests::linux_uninspectable_shared_listener_owner_is_not_exclusive` | `ok` inside the same binary; E02-B keeps incomplete peer visibility non-authorizing. |

The job ran its exact `cargo test --workspace` command on `ubuntu-latest`;
its retained log names both tests and the enclosing
250-passed, 0-failed binary. Earlier individual WSL filters belonged to the
superseded head and are not used as final acceptance evidence.

## Canonical gates

| Exact command | Exit | Result |
|---|---:|---|
| `cargo fmt --check` | 0 | Clean. |
| `cargo clippy -p ferric-cli --all-targets --all-features -- -D warnings` | 0 | No warnings. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 | No warnings. |
| `cargo test -p ferric-cli --all-features --locked` | 0 | 336 passed, 0 failed: 255 unit + 6 benchmark + 69 CLI + 3 lifecycle + 3 hygiene. |
| `cargo test --workspace --all-features --locked` | 0 | 1,089 passed, 0 failed, 4 intentionally ignored. |
| `rustup target add aarch64-unknown-linux-gnu` | 0 | Target standard library already installed and up to date. |
| `cargo check --workspace --target aarch64-unknown-linux-gnu --locked` | 0 | Workspace compile-check passed. |
| `cargo check -p ferric-cli --features lifecycle-fixture --all-targets --target aarch64-unknown-linux-gnu --locked` | 0 | Lifecycle feature/all-target compile-check passed. This is compile evidence only, not AArch64 runtime evidence. |

CI run
[33351978700](https://github.com/crussella0129/Animus_Ferric/actions/runs/33351978700)
independently passed all six jobs at the same head, including ordinary Ubuntu
and Windows workspace gates, both native lifecycle jobs, AArch64 compilation,
and `backend-openai` Clippy.

## Failed-attempt correction records

An initial restricted-sandbox workspace attempt reached the benchmark
integration suite before a child Python checker was denied with `Access is
denied (os error 5)`. The identical canonical command was rerun with ordinary
subprocess permission and passed 1,089 tests with four intentional ignores.
The package command separately passed 336/336. This is harness qualification,
not a product failure.

Before `44f36a2` was committed, the independent diff reviewer caught a real
test-harness flake in a default-parallel `cargo test -p ferric-cli --bin
ferric` run: 248/249 tests passed and
`windows_retained_process_handle_smoke` failed after it queried a numeric
HANDLE value after `LiveProcess` had dropped it. Windows may immediately reuse
that value for another parallel test, so the stale-value query could report a
valid but unrelated HANDLE. The correction makes `Drop` and a consuming
test-only probe share one `close_handle` path, checks the real `CloseHandle`
return directly, nulls the owned field after success, and never queries the
stale number. The corrected exact native smoke passed, followed by eight
consecutive default-parallel runs of 255/255 tests (2,040 executions), the
336/336 package suite, the 1,089-test workspace suite, and the successful
Windows CI jobs. The failed attempt is retained as a flake-oracle correction,
not represented as a production lifecycle failure.

Pre-merge adversarial review then found that a successful publication could
return `Ready` after persistence without a final retained-generation/listener
inspection. Commit `b679a25` adds that authority gate and deterministic
identity-transition/listener-transition compensation cases to
`up_spawned_child_binding_precedes_readiness`. All nineteen frozen filters,
all twenty-two Windows-applicable supplemental filters, the 336-test package
suite, the 1,089-test workspace suite, strict local gates, and six-job push and
pull-request CI matrices were re-run at the corrected head.
