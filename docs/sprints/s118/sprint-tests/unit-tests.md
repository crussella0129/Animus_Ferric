# Sprint 118 Unit and Composition Test Results

- **Tested code head:** `0145e45cb3ab8ab74ae71981d0851525eef2eb1c`
- **Executed:** 2026-08-31 on `x86_64-pc-windows-msvc`
- **Toolchain:** Rust/Cargo 1.96.0
- **Intent oracle:** [INT-0008 AC-3, AC-4, AC-6, AC-7, and enabling
  AC-9](../../../intents/INT-0008-unified-local-model-workflow.md#acceptance-criteria)

## Clause-level results

| EARS clause | Executed named evidence | Result |
|---|---|---|
| T-11801-E01 | `serve_status_projects_only_exact_token_path` | passed: absent/exact/unrelated/longest-path projection and canonical digest |
| T-11801-E02 | `serve_status_rejects_non_authorizing_shapes` | passed: command/timeout/size/schema/duplicate/non-proxy failures stay non-authorizing |
| T-11801-E03 | `serve_commands_are_closed_and_endpoint_scoped` | passed: exact apply/off argv; no reset, set-config, shell, root, or unscoped route |
| T-11801-E04 | `ownership_token_and_remote_base_are_valid`; `ownership_entropy_failure_precedes_side_effects`; `tailscale_pre_mutation_failures_never_apply` | passed: 128-bit coordinate and canonical bases; the production prepare-before-launch helper leaves its engine-spawn seam at zero for entropy and identity failure and records no mutation |
| T-11801-E05 | `runfile_schema_is_additive_and_validated`; `legacy_tailscale_registration_remains_unowned` | passed: typed schema authorizes; historical boolean-only state remains readable and fail-closed |
| T-11802-E01 | `tailscale_launch_orders_journal_before_apply`; `tailscale_launch_tolerates_unrelated_prestate_drift` | passed: ordered mirrored journal/apply/verify/final authority and unrelated drift tolerance |
| T-11802-E02 | `tailscale_pre_mutation_failures_never_apply` | passed: identity/status/collision/readiness/inspection/publication rows record no Serve mutation and exact compensation |
| T-11802-E03/E04 | `tailscale_launch_failure_matrix_holds_or_compensates_exactly` | passed: absent/owned apply error, malformed/replaced/post-off states, child/listener/revision drift, and off error hold or compensate exactly |
| T-11803-E01 | `status_reports_each_proxy_state` | passed: active/pending/replaced/uninspectable plus native ready/degraded gating and exact guidance |
| T-11803-E02 | `down_cleans_proxy_before_process` | passed: non-empty revision checks and exact proxy reconciliation precede retained-process teardown |
| T-11803-E03 | `down_proxy_failure_matrix_preserves_journal` | passed: six ambiguity/failure rows plus pre/post revision failures retain evidence and restrict process action |
| T-11803-E04 | `down_retries_absent_proxy_and_stale_process`; `tailscale_cli_lifecycle_preserves_unrelated_state` | passed: already-absent and crashed/exited composition rows converge; the real CLI repeats down against the first cleanup's resulting state without another Tailscale call or journal |
| T-11803-E05 | `legacy_tailscale_registration_remains_unowned` | passed for local/global/origin status and down: no process or Tailscale authority |
| T-11804-E01 | `doctor_tailscale_is_bounded_and_read_only` | passed: exact identity/status reads and precise missing/old/daemon/timeout/output/schema classifications |
| T-11804-E02 | `doctor_blockers_precede_all_probes` | passed: static and inventory blockers leave every external probe counter at zero |
| T-11804-E03 | `tailscale_operator_rendering_is_copy_paste_complete` | passed: local/remote bases, exact coordinate, retained evidence, scoped retry, and no blind reset |
| T-11805-E02 | `tailscale_fault_seam_clause_matrix` | passed: aggregate executes the launch/status/down/retry/revision/legacy clause matrices |

T-11805-E01 and E03 cross-process evidence is recorded in
[e2e-tests.md](e2e-tests.md).

## Frozen and regression commands

| Command | Final result |
|---|---|
| `cargo fmt --all -- --check` | passed |
| `cargo clippy -p ferric-cli --all-targets --all-features -- -D warnings` | passed |
| `cargo test -p ferric-cli --all-features tailscale_` | passed: 17, failed: 0, ignored: 1 timeout subprocess helper; the filter also passed all 3 Tailscale fixture tests |
| `cargo test -p ferric-cli --all-features server::tests` | passed: 73, failed: 0, ignored: 0 |
| `cargo test -p ferric-cli --all-features --test server_lifecycle_fixture` | passed: 5, failed: 0, ignored: 0 |
| `cargo test --workspace --all-targets --all-features` | passed with ordinary nested-process permission: 1,108 passed, 0 failed, 5 intentional helper ignores across 57 suite summaries |
| `cargo test -p ferric-cli --doc` | not applicable and exited 1: Cargo reported `no library targets found in package ferric-cli`; metadata confirms both package targets are binaries with `doctest = false` |
| supplemental `cargo test --workspace --doc` | passed across all 14 workspace library doc-test targets; 0 doctests exist |
| `git diff --check` | passed |

The package-specific doc command is retained as an exact failed frozen-command
record rather than rewritten after finalization. It cannot execute a doc test
for a binary-only package and therefore found no product failure or untested
Sprint 118 clause; the workspace doc surface is the applicable supplemental
gate.

## Corrected attempts

- The first restricted workspace run blocked the nested benchmark Python
  checker. The exact canonical workspace command was rerun with ordinary
  subprocess permission and passed 1,108/1,108 with five intentional ignores.
- The first help smoke found that adding the feature-gated fixture binary made
  `cargo run -p ferric-cli` ambiguous. Commit `35f16d5` restored `ferric` as the
  default run target; both frozen help commands then passed.
- A final-head workspace run found one concrete home-directory literal in a
  test assertion. Commit `411d437` replaced it with the documentation value;
  all 3 template-hygiene tests and the complete workspace then passed.
- The mandatory Test critic found that preparation tests could not observe an
  engine-spawn regression and that the repeated-down unit row bypassed real
  discovery. Commit `0145e45` routes production launch through the tested
  prepare-before-launch seam and repeats down through the stateful real CLI;
  every frozen executable gate was rerun at that corrected head.

## Accepted frozen-plan deviations

- The finalized test-plan description included a deterministic local-path
  absolutization failure row. `std::path::absolute` has no injectable failure
  seam and no portable invalid-path coordinate, so that one descriptive row
  was not executed. The governing T-11802-E02 EARS outcomes—capture,
  collision, readiness, exact inspection, and mirrored publication failure
  before Serve mutation—are all executed. The production local-path error
  branch still stops the retained child before returning. This is an explicit
  Test provenance caveat, not an assertion that the frozen row passed.
- Frozen command 7 remains red because the selected package has no library or
  doctest target. The applicable workspace doc surface passed and no Sprint
  118 clause relies on doctests, but the immutable command itself is not
  reported as green.

No GitHub CI conclusion exists before the sprint PR is opened. These are the
authoritative local Test-phase results for the recorded code head; remote CI
must be reported separately and cannot be inferred from them.
