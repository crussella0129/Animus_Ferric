Finalized - DO NOT EDIT

# Sprint 120 Test Plan

Approved proposal promoted without weakening its clauses or evidence boundaries.
See [Build plan](build-plan.md) for exact EARS and affected partial scope.

## Intent Traceability

These names are planned assertions, not already implemented tests. Each row
must receive its exact invocation, source location, result and immutable head
in Test evidence. Aggregate suite counts do not replace the clause matrix.

| Intent / criteria | Clause | Named verification |
|---|---|---|
| [INT-0005](../../../intents/INT-0005-safe-multilanguage-syntax-admission.md) AC-1/3/4/5 | E01-A | `python_05_admission_matrix`; `unsupported_codegen_remains_unchecked`; `syntax_check_has_no_external_side_effects` |
| [INT-0005](../../../intents/INT-0005-safe-multilanguage-syntax-admission.md) AC-2 | E01-B | `except_star_is_valid`; `controlled_mutation_python_05_transition_matrix` |
| [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md) AC-5/6 | E02-A | `config_absence_and_precedence`; `present_invalid_config_blocks_all_consumers`; `config_errors_redact_credentials`; `invalid_effective_numbers_rejected` |
| [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md) AC-6 | E02-B | `selected_workspace_drives_real_provider`; `explicit_endpoint_and_ambiguous_discovery_matrix` |
| [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md) AC-5/6 | E02-C | `chat_effective_stream_matrix`; `omitted_resume_harness_inherits` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-3/6/7/12 | E03-A | `startup_borrows_ready_server`; `startup_ambiguous_registration_is_nonmutating`; `startup_credentials_stay_endpoint_bound` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-6 | E03-B | `startup_owned_runtime_reaches_ready`; `startup_listener_identity_mismatch_refuses` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-6/7 | E03-C | `startup_cleanup_fault_matrix`; `startup_cancellation_reaps_scope`; `borrowed_server_survives_session_exit` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-3/10 | E03-D | `startup_concurrent_invocations_serialize`; `preferences_atomic_and_symlink_safe`; `stale_preferences_do_not_reuse_qualification` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-6/10/12 | E03-E | `startup_probe_limits_matrix`; `metadata_does_not_promote_authority` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-5 | E03-F | `startup_explain_is_read_only` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-1/12 | E04-A | `human_first_run_decision_budget`; `cargo_default_launch_selects_real_cli` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-12 | E04-B | `no_args_non_tty_welcome_is_nonmutating`; `malformed_explicit_commands_remain_usage_errors` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-6/12 | E04-C | `human_ask_never_dispatches`; `human_work_requires_scoped_consent`; `human_text_is_not_shell`; `new_session_does_not_inherit_edit_consent` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-10/12 | E04-D | `human_repeat_reuses_model`; `human_decline_eof_and_errors_are_bounded` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-1/11 | E04-E | `primary_help_is_compact`; `advanced_original_commands_compatible`; `no_default_features_welcome_and_mock_compatibility` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-6/12 | E05-A | `provider_cancellation_all_response_phases`; `human_cancel_during_request_reaps_owned_engine` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-12 | E05-B | `sse_unicode_every_split`; `sse_malformed_utf8_reports_error`; `sse_ascii_done_compatibility` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-9/12 | E06-A | `human_journey_e2e_matrix` (composes E02–E05, checks every fixture child reaped) |
| affected [INT-0005](../../../intents/INT-0005-safe-multilanguage-syntax-admission.md)/6/8 | E06-B | `source_quality_and_feature_matrix`; `first_run_docs_match_cli`; template-hygiene and existing command regression suites |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) AC-6/9/12 partial | E06-C | `real_model_prepared_host_journey` (source-owned live acceptance, separate from mock result) |

## Unit Tests

Pure loader/precedence/error classification, bounded model-selection and
presentation reducers, preference validation, permission/mode routing,
incremental SSE parsing, and injected Python compiler results cover the named
unit assertions above. Inject environment/path/clock/probe collaborators rather
than mutate global current directory or print secrets. For unreadable config,
include deterministic injected IO errors as well as an actual directory-at-file
fixture; do not rely only on platform permission bits.

## Integration Tests

Source-defined HTTP fixtures cover ready/stalled/redirected/oversized endpoints,
credentials and Unicode. Source-defined engine fixtures run only under Cargo-
driven tests, using the shared process owner and checked reaping. A real
concurrency barrier verifies one workspace startup winner; assertions cover
preference failure before/after publication, symlink refusal and borrowed
resource preservation. Selected-workspace tests use two explicit temp roots,
not global chdir. Existing query/MCP/API/chat/ICM configuration and resume tests
remain regression coverage, with protocol errors asserted in their own forms.

## End-to-End Tests

**Status: possible for this prepared-host increment.** The source-owned journey
matrix exercises the same orchestration and injected terminal IO used by the
real front door, plus Cargo-driven process-level no-argument/help/invalid-args
and native lifecycle cases. Input transcript assertions count model/authority/
resource decisions, reject technical prompts, and verify observed file effects.
Do not call a parser snapshot an interactive end-to-end test. Include actual
terminal input where the host supports it; retain transcript and explain any
terminal-fixture limitation rather than substituting non-TTY welcome success.

The live test uses an existing local model with no downloads and a maximum
180-second startup plus one 120-second conversation attempt. Evidence records
the actual duration and any failure. A source-owned test controls cancellation
and proves reaping; do not manually terminate process leftovers. A short response
is usability evidence, not proof of long-horizon coding accuracy.

Full clean-host acquisition/calibration, complete interrupted workflow resume,
native platform parity and model-built application qualification remain unlocked
by INT-0007/[INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)'s named work (T-11505–T-11509, T-11506/T-11410/T-11412),
not silently waived by this narrower feasible E2E scope.

## Quality Gates and Sprint Closeout

Run affected tests first after coherent changes, then `cargo fmt --all --check`,
workspace clippy with warnings denied, workspace tests with backend enabled,
CLI no-default-feature tests, and the lifecycle-fixture tests required by the
existing CI. Every child-producing command is source-aware and bounded. Retain
failed attempts, exact head/commands/results and cleanup evidence. Perform the
independent Test critique, Loop reconciliation and an additional independent
post-Loop adversarial pass. Normal task commits and implementation-head pushes
provide immutable CI evidence before final Test acceptance; they do not open a
PR. Only then make the final closeout commit/push, confirm remote head and
`origin/main..dev` contains this sprint alone, open one dev-to-main PR, verify
its commit count and required CI, restore/hash-check the protected user edit,
and stop for the owner to merge. Never merge the PR or label a failed live
qualification as successful sprint acceptance.
