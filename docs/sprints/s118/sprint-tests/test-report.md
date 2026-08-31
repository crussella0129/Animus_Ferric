# Sprint 118 Test Report

## Verdict

**Pass with two accepted Test-provenance caveats for Sprint 118's
ownership-safe Tailscale Serve scope.** At corrected implementation head
`0145e45cb3ab8ab74ae71981d0851525eef2eb1c`, every governing EARS outcome has
named executed evidence, the independent final [Test critique](critique.md)
returns `proceed-with-caveats`, the scoped Tailscale/server/E2E suites pass, and
the all-target/all-feature workspace passes 1,108 tests with five intentional
helper ignores.

The two caveats are not product failures and are not represented as passes:
the finalized test-plan description named a local-path-absolutization fault row
that cannot be injected portably through `std::path::absolute`, and frozen
command 7 selects package doc tests for a binary-only package with doctests
disabled. The applicable workspace doc-test surface passes.

This advances the affected server-lifecycle portions of INT-0008 AC-3, AC-4,
AC-6, and AC-7 and supplies model-free enabling evidence toward AC-9. It does
not complete the wider compact workflow, live-tailnet acceptance, hostile
compare/off CAS, AC-8 platform parity, macOS support, or a model-backed
medium-horizon application run. INT-0008 remains `active`, not `realized`.

## Acceptance result

| Task / EARS clause | Result | Named evidence |
|---|---|---|
| T-11801-E01 | pass | `serve_status_projects_only_exact_token_path` |
| T-11801-E02 | pass | `serve_status_rejects_non_authorizing_shapes` |
| T-11801-E03 | pass | `serve_commands_are_closed_and_endpoint_scoped` |
| T-11801-E04 | pass | `ownership_token_and_remote_base_are_valid`; `ownership_entropy_failure_precedes_side_effects`; production launch-closure assertions in `tailscale_pre_mutation_failures_never_apply` |
| T-11801-E05 | pass | `runfile_schema_is_additive_and_validated`; `legacy_tailscale_registration_remains_unowned` |
| T-11802-E01 | pass | `tailscale_launch_orders_journal_before_apply`; `tailscale_launch_tolerates_unrelated_prestate_drift` |
| T-11802-E02 | pass with descriptive-row caveat | `tailscale_pre_mutation_failures_never_apply` proves every governing EARS outcome; the separately described local-path standard-library fault row is the accepted deviation |
| T-11802-E03/E04 | pass | `tailscale_launch_failure_matrix_holds_or_compensates_exactly` |
| T-11803-E01 | pass | `status_reports_each_proxy_state` |
| T-11803-E02 | pass | `down_cleans_proxy_before_process` |
| T-11803-E03 | pass | `down_proxy_failure_matrix_preserves_journal` |
| T-11803-E04 | pass | `down_retries_absent_proxy_and_stale_process`; second real-CLI down in `tailscale_cli_lifecycle_preserves_unrelated_state` |
| T-11803-E05 | pass | `legacy_tailscale_registration_remains_unowned` |
| T-11804-E01 | pass | `doctor_tailscale_is_bounded_and_read_only` |
| T-11804-E02 | pass | `doctor_blockers_precede_all_probes` |
| T-11804-E03 | pass | `tailscale_operator_rendering_is_copy_paste_complete` |
| T-11805-E01 | pass, model-free | `tailscale_cli_lifecycle_preserves_unrelated_state` |
| T-11805-E02 | pass | `tailscale_fault_seam_clause_matrix` |
| T-11805-E03 | pass | `tailscale_command_log_contains_no_broad_mutation`; `tailscale_fixture_rejects_apply_without_journals` |

Detailed arrangement, assertion, and evidence-boundary records are retained in
[unit/composition](unit-tests.md), [integration](integration-tests.md), and
[end-to-end](e2e-tests.md) results.

## Canonical confirmations

- Formatting passed.
- Ferric CLI all-target/all-feature Clippy passed with warnings denied.
- The `tailscale_` filter passed 17 tests with one intentional subprocess
  helper ignore; its three matching lifecycle-fixture tests also passed.
- All 73 `server::tests` passed.
- All five serialized lifecycle E2Es passed, including three new Tailscale
  rows and the real repeated-down path.
- `cargo test --workspace --all-targets --all-features` passed with ordinary
  nested-process permission: 1,108 passed, 0 failed, five intentional helper
  ignores across 57 suite summaries.
- Frozen package doc command 7 exited 1 with `no library targets found`; the
  supplemental applicable `cargo test --workspace --doc` passed all 14 library
  targets, which contain zero doctests.
- `git diff --check` and both exact `cargo run -p ferric-cli -- server ...
  --help` smokes passed after restoring `ferric` as the package default run
  target.
- The protected Sprint 114 acquisition artifact remained unstaged with
  SHA-256
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`.

## Adversarial correction record

The initial mandatory Test critique returned `block`. It correctly found that
entropy/identity tests could not observe a production engine-spawn ordering
regression and that the purported repeated-down row supplied a synthetic empty
plan instead of consuming real post-cleanup state. Commit `0145e45` made the
production prepare-before-launch boundary own an observable launch closure and
added a second real CLI down with unchanged external command ledger, absent
journals, closed listener, and unchanged unrelated state. All executable gates
were rerun at that head. The second independent critique accepted those fixes
and retained only the two provenance caveats above.

Test execution also caught and corrected two pre-critic regressions: the
feature-gated fixture binary initially made `cargo run -p ferric-cli`
ambiguous (`35f16d5` restores the default target), and one fixture assertion
used a concrete home-directory form (`411d437` restores template hygiene).
Failed attempts remain in [unit test evidence](unit-tests.md#corrected-attempts)
rather than being hidden by the final pass.

## Environment and CI conclusion

Local execution used Rust/Cargo 1.96.0 on x86_64 Windows. The first restricted
workspace attempt blocked a nested benchmark Python child; the identical
canonical command passed with ordinary subprocess permission, matching the
already accepted harness qualification boundary from Sprint 117.

No Sprint 118 GitHub CI run exists before the sprint PR is opened. Therefore
the authoritative Test-phase conclusion is local at the recorded code head;
remote CI is pending and is not inferred or claimed here.
