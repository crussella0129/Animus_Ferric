# Sprint 118 Test Report

## Verdict

**Pass at corrected implementation head
`7633f8c0675664e51c8a4e88e4aaafe0d20880e9`.** The post-Loop adversarial
review rejected the first CLI-backed implementation, replaced that mutation
boundary with a direct Tailscale LocalAPI ETag/`If-Match` CAS, and reran the
applicable frozen and supplemental gates. The corrected LocalAPI suite passed
19/19, the Serve projection/mutation suite passed 17/17, all 84 server-filter
tests passed, and all five serialized lifecycle fixtures passed.

The requested post-evidence five-phase audit initially found additional P2s,
reopened Loop, and required ancestor-route, future-version status, pinned
fresh-CAS validation, operator wording, and Book provenance corrections. PR CI
then forced a third Loop re-entry: the test-only TCP seam leaked dead-code
items into default/backend feature matrices, and the first Linux namespace
topology ran the Rust harness as PID 1, leaving an adopted managed child as a
zombie between tests. The corrected unprivileged PID-1 reaper passed on both
operating systems; follow-up review also restored `PDEATHSIG` across the
credential drop and corrected an apostrophe that broke the outer CI shell's
single-quoted program. Final independent re-review found no remaining P0, P1,
or P2 issue. P3 limitations
remain explicit: no successful native Linux UDS or Windows pipe exchange, a
narrow Unix two-step `CLOEXEC` fork-inheritance window, no live-tailnet run,
the upstream status-identity/Serve-ETag atomicity gap, and imprecise precedence
when several effective ancestor diagnostics coexist. None is represented as
tested away. The Test critique separately retains P3 CI-wrapper maintenance,
runner-shell, and signal-propagation qualifications.

This advances the affected parts of INT-0008 AC-3, AC-4, AC-6, and AC-7 and
provides model-free enabling evidence toward AC-9. It does not realize the
wider compact workflow, live-tailnet acceptance, AC-8 platform parity, macOS
LocalAPI support, or a model-backed medium-horizon application run. INT-0008
therefore remains `active`.

## Acceptance result

The finalized plans remain immutable provenance. CLI-specific mechanism text
discovered to be unsafe during Loop is marked **superseded**, not retroactively
reported as passing unchanged.

| Task / EARS clause | Corrected result | Named evidence |
|---|---|---|
| T-11801-E01/E02 | semantic outcome passed through direct LocalAPI projection; the planned CLI status transport was superseded | `localapi_apply_rejects_activation_and_schema_hazards`; `localapi_descendant_blocks_publication_but_cleanup_preserves_it`; `localapi_cleanup_preserves_alias_and_unrelated_host_token`; `duplicate_json_keys_are_rejected_recursively`; `response_header_and_body_caps_are_enforced` |
| T-11801-E03 | **locked CLI argv clause superseded** by a narrower request API and exact CAS; the old CLI test name was not executed | `exact_request_headers_and_cas_etag`; `serve_cas_412_is_typed_no_mutation`; `post_send_timeout_is_indeterminate`; `tailscale_localapi_log_contains_no_broad_mutation_or_retry` |
| T-11801-E04/E05 | passed | `ownership_token_and_remote_base_are_valid`; `ownership_entropy_failure_precedes_side_effects`; `runfile_schema_is_additive_and_validated`; `mirrored_tailscale_provenance_conflicts_block_before_effects` |
| T-11802-E01 | passed | `tailscale_launch_orders_journal_before_apply`; `tailscale_launch_tolerates_unrelated_prestate_drift` |
| T-11802-E02 | **passed for the injectable rows, with the frozen local-path-resolution row still descriptive** | `tailscale_pre_mutation_failures_never_apply`; T-11806 owns the missing deterministic local registration-path resolution seam before exhaustive fault coverage is claimed |
| T-11802-E03/E04 | passed | `tailscale_launch_failure_matrix_holds_or_compensates_exactly`; `tailscale_identity_races_never_publish_or_cross_profile_cleanup`; `tailscale_cleanup_allows_same_node_rename_without_https_authority` |
| T-11803-E01 | passed | `status_reports_each_proxy_state`; `status_exact_proxy_is_not_ready_after_tailscale_identity_drift`; `localapi_descendant_blocks_publication_but_cleanup_preserves_it`; `localapi_cleanup_preserves_alias_and_unrelated_host_token` |
| T-11803-E02/E03/E04/E05 | passed | `down_cleans_proxy_before_process`; `down_proxy_failure_matrix_preserves_journal`; `down_retries_absent_proxy_and_stale_process`; `legacy_tailscale_registration_remains_unowned` |
| T-11804-E01 | **locked CLI probe clause superseded** by bounded, read-only LocalAPI status/config/status sessions; the old CLI transport was not executed | `doctor_tailscale_is_bounded_and_read_only`; `status_binding_uses_stable_id_and_https_capability`; `session_reuses_one_connection_for_status_serve_status` |
| T-11804-E02/E03 | passed | `doctor_blockers_precede_all_probes`; `tailscale_operator_rendering_is_copy_paste_complete` |
| T-11805-E01 | **locked fake-CLI fixture clause superseded** by a real Ferric process against a stateful fake LocalAPI | `tailscale_localapi_lifecycle_preserves_unrelated_state` |
| T-11805-E02 | passed | `tailscale_fault_seam_clause_matrix` and the final 84-test server substring filter, including six `api::server::tests` |
| T-11805-E03 | **locked argv-ledger clause superseded** by an exact HTTP request/CAS ledger; the old CLI log test was not executed | `tailscale_localapi_log_contains_no_broad_mutation_or_retry`; `ordinary_ferric_ignores_lifecycle_localapi_override` |

Detailed arrangements and evidence boundaries are retained in the
[unit/composition](unit-tests.md), [integration](integration-tests.md), and
[end-to-end](e2e-tests.md) records.

## Canonical confirmations

- `cargo fmt --all -- --check` passed.
- Workspace all-target/all-feature Clippy passed with warnings denied.
- Exact-head CI also passed default Clippy on Ubuntu and Windows,
  `backend-openai` Clippy on Ubuntu, and `lifecycle-fixture` Clippy on Ubuntu
  and Windows.
- LocalAPI tests passed 19/19; Serve tests passed 17/17; the server substring
  filter passed 84/84 (including six `api::server::tests`); lifecycle fixtures
  passed 5/5.
- The exact frozen `cargo test -p ferric-cli --all-features tailscale_`
  command passed 55 unit tests plus 2 lifecycle tests; all other selected
  targets ran 0 tests.
- `cargo test --workspace --all-targets --all-features` passed outside the
  restricted sandbox. The restricted attempt was unable to qualify the nested
  Python benchmark child; the identical command with ordinary child-process
  permission is the authoritative result.
- Applicable workspace doc tests and both operator help smokes passed.
- The default-feature `ferric` aarch64 Linux check passed. The all-feature
  aarch64 attempt reached `ring` and was blocked only because the host lacks
  `aarch64-linux-gnu-gcc`; it produced no Ferric diagnostic.
- `git diff --check` passed before evidence reconciliation.
- The protected Sprint 114 acquisition artifact remained unstaged with
  SHA-256
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`.

The frozen package-specific command `cargo test -p ferric-cli --doc` exited 1
with `error: no library targets found in package ferric-cli` because
`ferric-cli` contains binaries and integration tests with doctests disabled.
The applicable supplemental command `cargo test --workspace --doc` passed.
This preserves the immutable command result; it does not relabel that command
green.

## Post-Loop correction and review

The mandatory extra adversarial pass found that the first implementation's
native CLI compare/modify boundary could not supply the hostile concurrency or
completion semantics required by the ownership claim. It also exposed
identity-switch, route-shadow, shared-scaffold, version-drift, transport, and
fixture-isolation cases that the earlier argv ledger could not prove. The Loop
was reopened rather than PRing that implementation.

Final tested head `7633f8c` includes the LocalAPI correction begun at
`625fbba`, plus the mandatory final-audit fixes. It uses a bounded client over the
Linux Unix-domain socket or Windows protected named pipe, pins the normal
capability/version contract, validates duplicate-safe status and Serve JSON,
binds configuration reads to same-session identity sandwiches, verifies the
raw-body SHA-256 ETag, sends at most one exact `If-Match` POST, treats HTTP 412
as definite no-mutation, and treats post-send failures as indeterminate. It
also adds handler-only compatible-version cleanup, descendant/alias/ancestor
route-shadow retention, strict pinned cleanup snapshots, future-semantics
status refusal, stable-node/FQDN lifecycle rules, scaffold provenance, and an
isolated test-only TCP endpoint available only to `ferric-lifecycle-test`.

The complete correction and five-phase audit are recorded in the
[post-Loop adversarial review](../post-loop-adversarial-review.md). The final
independent re-reviews found no remaining P0-P2 issue and retained only the P3
limitations named above.

PR run `33385435515` exposed the third re-entry at evidence head `85f5e5b`:
three Clippy configurations rejected lifecycle-only dead code, and the Linux
lifecycle job passed only 3/5 after namespace PID 1 failed to reap a detached
managed child. Commit `2f976dc` narrowed the cfg boundary and made an
unprivileged shell the namespace reaper; push/PR runs `33387648205` and
`33387653011` passed, but that head was superseded when review found that the
credential transition could clear `PDEATHSIG`. Commit `a4bf920` restored that
signal contract with `setpriv --pdeathsig keep`; its runs `33388127765` and
`33388132395` then caught a shell-quoting defect caused by the apostrophe in a
comment. Commit `7633f8c` repaired the quoting. The exact corrected wrapper
passed all 5 tests locally in an isolated PID/network/proc namespace before
the final exact-head push and PR CI runs were accepted.

## Environment and CI conclusion

Local execution used Rust/Cargo 1.96.0 on x86_64 Windows, with the corrected
Linux lifecycle wrapper rerun under WSL in an isolated PID/network/proc
namespace as unprivileged UID/GID 1000. GitHub push run `33388704624` and PR
run `33388709925` both completed successfully at the same final code head
across Ubuntu and Windows. The subsequent evidence-only commit must also pass
CI before merge.
