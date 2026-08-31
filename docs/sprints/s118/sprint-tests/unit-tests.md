# Sprint 118 Unit and Composition Test Results

- **Tested code head:** `d5e61b7f951ca838ea2aed7cefaa2468282bb164`
- **Executed:** 2026-08-31 on `x86_64-pc-windows-msvc`
- **Toolchain:** Rust/Cargo 1.96.0
- **Intent oracle:** [INT-0008 AC-3, AC-4, AC-6, AC-7, and enabling
  AC-9](../../../intents/INT-0008-unified-local-model-workflow.md#acceptance-criteria)

## Direct LocalAPI boundary — 19/19 passed

The LocalAPI suite exercises the transport and HTTP/CAS authority directly:

- exact request headers, raw-body SHA-256 ETags, exact `If-Match`, duplicate
  header rejection, strict capability/version status binding, and same-session
  connection reuse;
- bounded content-length/chunked/close-delimited bodies, bounded headers,
  deadline behavior, conflicting framing rejection, duplicate-safe recursive
  JSON parsing, and linear bounded certificate-domain validation;
- typed HTTP-412 no-mutation, post-send timeout/access-denial indeterminacy,
  and no retry after mutation bytes may have escaped;
- test-endpoint loopback restriction, Windows named-pipe retry classification,
  and a native pending-read timeout test proving bounded return and poisoned
  non-reuse.

Named evidence includes `exact_request_headers_and_cas_etag`,
`status_binding_uses_stable_id_and_https_capability`,
`daemon_identity_headers_are_unique_and_exact_for_apply`,
`serve_cas_412_is_typed_no_mutation`,
`serve_cas_access_denial_after_post_is_indeterminate`,
`post_send_timeout_is_indeterminate`,
`named_pipe_pending_read_timeout_is_bounded_and_poisoned`, and
`session_reuses_one_connection_for_status_serve_status`.

## Serve projection and mutation — 17/17 passed

The Serve suite proves exact handler ownership while preserving shared state:

- identity is bound by StableNodeID/FQDN for publication, while cleanup allows
  a same-node rename and never crosses to a different profile;
- apply/off round trips pristine state and preserves supported unrelated
  handlers, services, Funnel data, foreground dependencies, and existing or
  concurrently added scaffolding;
- activation hazards, descendant routes, trailing-slash aliases, unrelated
  host/token paths, replacement targets, and effective route shadows fail
  closed or retain evidence as specified;
- pinned cleanup recognizes `/`, `//`, `/_ferric`, and `/_ferric/` ancestors
  when the exact handler is absent, keeps them latent while it is active, and
  strictly revalidates both the observation and fresh CAS snapshot;
- compatible version-drift cleanup removes only the exact handler, retains all
  scaffolding, and refuses a rewrite that would alter an unknown numeric
  lexeme.

Named evidence includes `localapi_apply_and_off_round_trip_pristine_config`,
`localapi_apply_preserves_supported_unrelated_state`,
`localapi_apply_rejects_activation_and_schema_hazards`,
`localapi_descendant_blocks_publication_but_cleanup_preserves_it`,
`localapi_cleanup_preserves_alias_and_unrelated_host_token`,
`localapi_off_preserves_preexisting_and_concurrent_scaffolding`,
`localapi_off_keeps_created_https_listener_for_each_live_dependency`,
`localapi_cleanup_detects_and_preserves_effective_ancestor_handlers`,
`pinned_cleanup_observation_reports_ancestors_and_rejects_unknown_fields`,
`pinned_localapi_off_revalidates_the_fresh_cas_snapshot`,
`version_drift_cleanup_removes_only_handler_and_never_scaffolding`, and
`forward_cleanup_never_rewrites_unknown_number_lexemes`.

## Server composition — 84/84 passed

The server-module suite retains the frozen semantic outcomes for write-ahead
journaling, apply verification, exact-child compensation, truthful status,
proxy-first down, retry convergence, legacy refusal, and operator guidance. It
also adds direct checks for mirrored scaffold-provenance conflicts,
StableNodeID/FQDN races, same-node rename cleanup, identity-drift status, route
shadows, and confirmation-phase tears.

Key named evidence:

- `mirrored_tailscale_provenance_conflicts_block_before_effects`;
- `status_exact_proxy_is_not_ready_after_tailscale_identity_drift`;
- `status_reports_absent_ancestor_route_as_uninspectable` and
  `status_future_version_exact_proxy_is_uninspectable`;
- `tailscale_launch_orders_journal_before_apply`;
- `tailscale_pre_mutation_failures_never_apply`;
- `tailscale_identity_races_never_publish_or_cross_profile_cleanup`;
- `tailscale_cleanup_allows_same_node_rename_without_https_authority`;
- `tailscale_launch_failure_matrix_holds_or_compensates_exactly`;
- `down_cleans_proxy_before_process` and
  `down_proxy_failure_matrix_preserves_journal`;
- `phase_torn_tailscale_mirrors_promote_once_and_clean_fresh_bytes` and
  `partial_tailscale_confirmation_holds_every_journal_before_off`.

## Frozen clauses superseded during Loop

The plan files are intentionally unchanged. The following mechanism-specific
claims were not executed under their obsolete CLI names:

| Locked clause | Accurate corrected evidence |
|---|---|
| T-11801-E03 fixed apply/off argv | Superseded by a closed LocalAPI GET/POST surface, exact raw-body ETag, one `If-Match` CAS, no retry, and no generic command or shell API. |
| T-11804-E01 `whoami --json` / `serve status --json` | Superseded by bounded read-only LocalAPI status/config/status sessions with capability/version and identity binding. |
| T-11805-E01 fake CLI executables | Superseded by real Ferric processes against a stateful loopback fake LocalAPI. |
| T-11805-E03 argv log | Superseded by an exact method/path/header/body/CAS request ledger and a production-binary seam-isolation test. |

This is a stronger implementation of the ownership outcome, but it is a plan
deviation, not a retroactive pass for the obsolete transport.

## Commands and qualifications

| Command / gate | Final result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| workspace all-target/all-feature Clippy with `-D warnings` | passed |
| LocalAPI focused suite | passed: 19, failed: 0 |
| Serve focused suite | passed: 17, failed: 0 |
| `cargo test -p ferric-cli --all-features server::tests` | passed: 84, failed: 0, ignored: 0 |
| frozen `cargo test -p ferric-cli --all-features tailscale_` | passed: 55 unit plus 2 lifecycle; all other selected targets ran 0 tests |
| `cargo test -p ferric-cli --test server_lifecycle_fixture --all-features -- --test-threads=1 --nocapture` | passed: 5, failed: 0, ignored: 0 |
| `cargo test --workspace --all-targets --all-features` | passed outside the restricted sandbox; the restricted attempt could not qualify its nested Python child |
| `cargo test --workspace --doc` | passed |
| default-feature `ferric` check for `aarch64-unknown-linux-gnu` | passed |
| all-feature aarch64 check | environment-blocked at `ring`: `aarch64-linux-gnu-gcc` is not installed; no Ferric diagnostic |
| `git diff --check` | passed before evidence reconciliation |

The immutable package-specific command `cargo test -p ferric-cli --doc` exited
1 with `error: no library targets found in package ferric-cli`; the package is
binary/integration-only and has doctests disabled. The applicable supplemental
`cargo test --workspace --doc` gate passed. This preserves the frozen command's
exact result instead of relabeling it green.

No GitHub CI conclusion exists before the sprint PR. These are the
authoritative local results for the frozen code head; remote CI is pending.
