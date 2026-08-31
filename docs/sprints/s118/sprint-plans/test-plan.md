Finalized - DO NOT EDIT

# Sprint 118 Test Plan

## Intent Traceability

| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|---|---|---|---|
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-3 explicit partial/stale state | T-11801-E01/E02/E05; T-11802-E03/E04; T-11803-E01/E03/E05 | `serve_status_projects_only_exact_token_path`; `serve_status_rejects_non_authorizing_shapes`; `legacy_tailscale_registration_remains_unowned`; `tailscale_launch_failure_matrix_holds_or_compensates_exactly`; `status_reports_each_proxy_state`; `down_proxy_failure_matrix_preserves_journal` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-4 truthful status | T-11803-E01; T-11804-E01/E02/E03 | `status_reports_each_proxy_state`; `doctor_tailscale_is_bounded_and_read_only`; `doctor_blockers_precede_all_probes`; `tailscale_operator_rendering_is_copy_paste_complete` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-6 exact ownership and atomic evidence | T-11801-E03/E04/E05; T-11802-E01/E02/E03/E04; T-11803-E02/E03/E05; T-11805-E01/E03 | `serve_commands_are_closed_and_endpoint_scoped`; `ownership_token_and_remote_base_are_valid`; `ownership_entropy_failure_precedes_side_effects`; `runfile_schema_is_additive_and_validated`; `tailscale_launch_orders_journal_before_apply`; `tailscale_launch_tolerates_unrelated_prestate_drift`; `tailscale_pre_mutation_failures_never_apply`; `tailscale_launch_failure_matrix_holds_or_compensates_exactly`; `down_cleans_proxy_before_process`; `down_proxy_failure_matrix_preserves_journal`; `legacy_tailscale_registration_remains_unowned`; `tailscale_cli_lifecycle_preserves_unrelated_state`; `tailscale_command_log_contains_no_broad_mutation` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-7 scoped/idempotent cleanup | T-11801-E03; T-11802-E03/E04; T-11803-E02/E03/E04/E05; T-11804-E03; T-11805-E01/E03 | `serve_commands_are_closed_and_endpoint_scoped`; `tailscale_launch_failure_matrix_holds_or_compensates_exactly`; `down_cleans_proxy_before_process`; `down_proxy_failure_matrix_preserves_journal`; `down_retries_absent_proxy_and_stale_process`; `legacy_tailscale_registration_remains_unowned`; `tailscale_operator_rendering_is_copy_paste_complete`; `tailscale_cli_lifecycle_preserves_unrelated_state`; `tailscale_command_log_contains_no_broad_mutation` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | enabling AC-9 lifecycle E2E | T-11804-E01/E02/E03; T-11805-E01/E02/E03 | `doctor_tailscale_is_bounded_and_read_only`; `doctor_blockers_precede_all_probes`; `tailscale_operator_rendering_is_copy_paste_complete`; `tailscale_cli_lifecycle_preserves_unrelated_state`; `tailscale_fault_seam_clause_matrix`; `tailscale_command_log_contains_no_broad_mutation` |

## Unit Tests

### T-11801 adapter and schema

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `serve_status_projects_only_exact_token_path` (T-11801-E01): absent, exact owned, unrelated handlers, longest path, and stable canonical digest fixtures produce one exact typed projection.
- `serve_status_rejects_non_authorizing_shapes` (T-11801-E02): command failure/timeout/oversize, malformed root/Web/Handlers, duplicate token path, non-proxy handler, and wrong target fail without mutation.
- `serve_commands_are_closed_and_endpoint_scoped` (T-11801-E03): apply/off argv are exact and the adapter API has no reset, set-config, shell, root-path, or unscoped-off route.
- `ownership_token_and_remote_base_are_valid` (T-11801-E04): injected 128-bit entropy yields the strict token/mount/target and canonical self identity yields `https://example-host.tailnet-example.ts.net/_ferric/<token>/v1` while the ordinary runfile base remains loopback-local.
- `ownership_entropy_failure_precedes_side_effects` (T-11801-E04): OS entropy and self-identity failures advance neither engine nor Serve effect counters and return upgrade/entropy diagnostics.
- `runfile_schema_is_additive_and_validated` (T-11801-E05): new records round-trip; old false records deserialize; boolean-only true, invalid token/path/target/port/digest, and metadata disagreement remain non-authorizing.

### T-11802 launch composition

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `tailscale_launch_orders_journal_before_apply` (T-11802-E01): event ledger proves health/identity/listener → mirrored journal → pre-state recheck → exact apply → exact proxy verify → final native/revision recheck → ready.
- `tailscale_launch_tolerates_unrelated_prestate_drift` (T-11802-E01): an unrelated handler added between baseline and the pre-apply recheck remains byte-equivalent in the fake state, does not authorize or block the absent token coordinate, and survives apply/down.
- `tailscale_pre_mutation_failures_never_apply` (T-11802-E02): capture/collision/readiness/identity/listener/local-path/publication fault table records zero mutation and exact existing child/publication compensation.
- `tailscale_launch_failure_matrix_holds_or_compensates_exactly` (T-11802-E03/E04): apply-failed-but-absent, apply-failed-but-owned, malformed verification, replacement, off failure, absence-verification failure, child exit, listener drift, and registration replacement prove proxy-first compensation or held recovery with no unauthorized signal/removal.

### T-11803 status and down composition

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `status_reports_each_proxy_state` (T-11803-E01): active/pending/replaced/uninspectable proxy states produce exact success, state, remote base, and next-action output alongside native state.
- `down_cleans_proxy_before_process` (T-11803-E02): active and absent cases prove two revision checks, exact off plus absence verification when needed, then retained-handle signal/wait/listener proof and conditional file removal.
- `down_proxy_failure_matrix_preserves_journal` (T-11803-E03): replaced/duplicate/malformed/unreadable/off-failed/post-off-unreadable cases perform no ambiguous proxy mutation, may stop only an independently authorized exact process, perform zero registration cleanup, and retain every journal.
- `down_retries_absent_proxy_and_stale_process` (T-11803-E04): pending crash, already-off, already-exited, and repeated down converge without broad mutation.
- `legacy_tailscale_registration_remains_unowned` (T-11801-E05/T-11803-E05): boolean-only true local/global/origin records remain byte-identical, invoke neither process nor Tailscale effects, and render manual exact-coordinate recovery.

### T-11804 doctor and rendering

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `doctor_tailscale_is_bounded_and_read_only` (T-11804-E01): valid input runs engine/model plus exact `whoami --json` and `serve status --json` only, and reports missing/old binary, identity failure, daemon/status failure, malformed output, or success precisely.
- `doctor_blockers_precede_all_probes` (T-11804-E02): invalid port/context/model flags and blocked inventory return before engine/model/network/Tailscale effect counters advance.
- `tailscale_operator_rendering_is_copy_paste_complete` (T-11804-E03): launch/status/down rendering includes local and remote bases, token coordinate, retained journal path, scoped next action, no reset guidance, and no machine identity.

## Integration Tests

### Server lifecycle composition

- **Intents:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `tailscale_fault_seam_clause_matrix` (T-11805-E02): deterministic fake process/listener/publication/Serve/revision effects execute every T-11802 and T-11803 fault row and assert ordered events, terminal classification, signals, files, and diagnostics.
- Existing `server` module, `server_resolution`, `server_registration`, backend discovery, and adoption suites remain green to prove the additive schema and typed exception do not weaken non-Tailscale or legacy behavior.

## End-to-End Tests

- **Status:** possible (model-free fake executables); live-tailnet acceptance remains not-yet-possible in this sprint and is explicitly owned by [INT-0008 AC-9](../../../intents/INT-0008-unified-local-model-workflow.md#acceptance-criteria), requiring a separately authorized environment with Tailscale HTTPS/MagicDNS/ACL access before that criterion can complete. Hostile replacement of the exact high-entropy token during the native CLI compare/off window remains owned by INT-0008 AC-6 and requires a future LocalAPI `If-Match` adapter. AC-8 platform parity remains unlocked by T-11707.
- `tailscale_cli_lifecycle_preserves_unrelated_state` (T-11805-E01): the real `ferric` executable runs doctor → up → status → down with feature-gated fake engine/Tailscale executables; assertions cover write-ahead registration bytes, tokenized remote `/v1`, unrelated handler preservation, proxy-before-process teardown, exact exit, and local/global cleanup.
- `tailscale_command_log_contains_no_broad_mutation` (T-11805-E03): the complete fake-CLI invocation ledger contains only exact bounded `whoami --json`, `serve status --json`, token apply, and token off argv and rejects `reset`, `set-config`, shell invocation, root path, or unscoped `off`.
- Existing hardened Windows/Linux lifecycle fixture tests remain green. The protected Sprint 114 acquisition artifact hash is rechecked after all tests.

## Frozen Commands

1. `cargo fmt --all -- --check`
2. `cargo clippy -p ferric-cli --all-targets --all-features -- -D warnings`
3. `cargo test -p ferric-cli --all-features tailscale_`
4. `cargo test -p ferric-cli --all-features server::tests`
5. `cargo test -p ferric-cli --all-features --test server_lifecycle_fixture`
6. `cargo test --workspace --all-targets --all-features`
7. `cargo test -p ferric-cli --doc`
8. `git diff --check`
9. `cargo run -p ferric-cli -- server up --help`
10. `cargo run -p ferric-cli -- server doctor --help`
11. Verify SHA-256 of `docs/sprints/s114/control-artifacts/model/acquisition-tests.json` remains `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB` and the file is unstaged.

The command list is immutable after plan finalization. Test Phase records exact
pass/fail/ignored counts and environment limitations rather than inflating
model-free evidence into live-tailnet or AC-8 platform acceptance.
