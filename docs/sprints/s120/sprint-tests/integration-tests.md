# Sprint 120 clause-level integration map

## Status and evidence boundary

**Current qualified source: `4f4e4f04d4ee132f9df9bb422be88a5ce366915d`.
Fresh local native and both eight-job CI runs passed; renewed formal critique:
`proceed-with-caveats` for controlled test scheduling.** Prior acceptance at `0ec5a0e` remains historical after the
checkpoint timeout recurrence. This is an assertion inventory and invocation map,
not a replacement for the locked [test plan](../sprint-plans/test-plan.md) or
[build clauses](../sprint-plans/build-plan.md). Aggregate pass counts do not
prove every clause.

The current candidate executes every named E01–E06 assertion below through the
full default-backend canonical workspace gate (75 native suite confirmations)
and the unchanged backend-free matrix, with L explicitly run separately at the
same head. The exact current invocations/results are in the appended unit and
E2E sections. Per-key invocations recorded below at earlier heads remain their
own historical focused evidence, not falsely relabeled new executions. The
source clauses/argv/cleanup assertions are unchanged; diagnostics and controlled
Windows inter-test scheduling add qualification observability. Explicit startup
worker/barrier races still run concurrently. See [checkpoint diagnosis](checkpoint-diagnosis.md)
for the C-002 mitigation and unresolved historical cause/parallel-load boundary.

[Unit/affected-package evidence](unit-tests.md) records earlier Build checks and
final corrected-head results. [E2E evidence](e2e-tests.md) separately binds each
live/terminal attempt, including fresh final-head L/TTY. [CI evidence](ci-results.md)
retains every canonical suite confirmation and each failed predecessor. No old
candidate result is promoted to a later head. Failure remains failure; manual
process termination cannot repair a run into success.

Source references below are repository-relative `path::function` coordinates.
They avoid volatile line numbers; subsequent unqualified function names in a
group use the immediately preceding file path. All originally planned assertion
names are present. Additional assertions are identified separately; their
presence in source is not an execution result.

## Invocation keys

Commands run from the repository root. Normal defaults include the real
backend. Positive startup/session fixtures on Linux require the isolated
workspace mode; do not infer ordinary-host listener authority from that mode.

Rows H/HU/S/P/PY/M/CLI retain their **historical focused invocations at
`0ec5a0eb0f465e8220b7f2010428aed3d6f2975d`** and timings. Every other row below
records fresh qualification at `4f4e4f04d4ee132f9df9bb422be88a5ce366915d`.
At that current head, all H/HU/S/P/PY/M/CLI assertions also ran inside W-WIN
and W-LINUX; they were not separately refiltered and relabeled new focused runs.

| Key | Source-aware invocation | Result (head policy above) |
|---|---|---|
| W-WIN | `cargo test --workspace --locked -- --test-threads=1` on native Windows | Local and both native CI runs passed: 1,247 / 7 intentional ignores |
| W-LINUX | `bash tools/test-lifecycle-linux.sh workspace` in Linux CI | Native isolated Linux passed: 1,253 / 5 intentional ignores |
| N | `cargo test -p ferric-cli --no-default-features --locked` | Native Windows local and both CI hosts passed: 407 / 0 ignored each |
| H | `cargo test --locked -p ferric-cli --test human_cli --test human_docs --test source_execution --test template_hygiene` | Passed 14: human 8, docs 1, source 2, hygiene 3 |
| HU | `cargo test --locked -p ferric-cli --bin ferric human:: -- --test-threads=1` on native Windows | Passed 17 / 1 opt-in live ignore, 2.66 seconds; L executed separately |
| S | `cargo test --locked -p ferric-cli --features backend-openai --bin ferric startup:: -- --test-threads=1` on native Windows | Passed 38 / 0 ignored, 13.54 seconds |
| P | `cargo test --locked -p ferric-provider --features backend-openai --lib` | Passed 47 / 0 ignored, 0.71 seconds |
| PY | `cargo test --locked -p ferric-tools --lib check_syntax` | Passed 16 / 0 ignored |
| M | `cargo test --locked -p ferric-tools --test controlled_mutations` | Passed 15 / 0 ignored |
| CLI | `cargo test --locked -p ferric-cli --test cli` | Passed separate rerun: 72 / 0 ignored, 17.53 seconds |
| L | `cargo test --locked -p ferric-cli --bin ferric real_model_prepared_host_journey -- --ignored --exact human::enabled::tests::real_model_prepared_host_journey --nocapture --test-threads=1` | Passed 1 / 0 ignored, 7.02 seconds; see current-head E2E timings/cleanup |
| TTY | Actual terminal `cargo r -- run --workspace "<fresh-temporary-workspace>" --model "<existing-local-model.gguf>"`, then the bounded Ask transcript | Passed actual native terminal: expected answer, /quit, checked source cleanup, exit 0; see E2E |
| F | `cargo fmt --all --check` | Passed |
| FI | `rustfmt --edition 2024 --check crates/ferric-cli/src/human_journey_tests.rs` | Passed; included fixture checked explicitly |
| C | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| CN | `cargo clippy -p ferric-cli --no-default-features --all-targets --locked -- -D warnings` | Passed |
| LF-WIN | `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1` | Local passed 5 / 0 ignored, 20.34 seconds; both native CI runs passed 5 / 0 ignored |
| LF-LINUX | `bash tools/test-lifecycle-linux.sh` | Native isolated Linux passed 6 / 0 ignored |

Authoritative current-head [push run 33949875039](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039)
and [PR run 33949876363](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363)
each passed all eight jobs at `4f4e4f0`, including backend clippy and both
backend-enabled ARM64 compile checks. Local W-WIN/N used the output-only
`--quiet` option; CI ran the listed non-quiet commands. Historical
[CI run 33947290181](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181)
belongs to `0ec5a0e`, as do the separately executed focused H/HU/S/P/PY/M/CLI
numbers retained above. It is not evidence for the later candidate's CI state.

W-LINUX first warms source with `cargo test --workspace --locked --no-run`,
then runs `cargo test --workspace --locked --offline -- --test-threads=1`
inside the non-root PID/network namespace. The source reaper remains PID 1;
it does not extract or directly invoke Cargo artifacts. The existing lifecycle
mode remains a separate required gate. CI's aarch64 and explicit backend checks
also remain required; compilation is not native runtime evidence.

For L, explicitly set `FERRIC_LIVE_MODEL` to one already available GGUF. No
model/engine acquisition is part of the test. Retain the actual model/runtime,
CPU/context/temperature, decisions, timing, transcript and checked cleanup.
The startup bound is 180 seconds and the conversation attempt is 120 seconds;
the live test must not be replaced by a mock or non-TTY welcome result.

N does not enable `backend-openai` or `lifecycle-fixture`. The startup module
and positive human journey fixtures are backend-gated. Backend-free welcome,
mock commands and static contracts remain exercised without those fixtures.

## E01: Python 0.5 admission

Affected intent: [INT-0005](../../../intents/INT-0005-safe-multilanguage-syntax-admission.md),
Python maintenance only; other language admission is not newly qualified here.

| Clause / criteria | Exact source assertions | What the assertions establish / invocation keys |
|---|---|---|
| E01-A; AC-1/3/4/5 | `crates/ferric-tools/src/builtin/check_syntax.rs::python_05_admission_matrix`, `unsupported_codegen_remains_unchecked`, `syntax_check_has_no_external_side_effects` | Invalid contextual control flow rejects; injected NotImplementedYet remains unchecked; no candidate files appear. The unsupported-codegen assertion also names the actual `rustpython-compiler/0.5` diagnostic identity. PY, W-WIN, W-LINUX. |
| E01-A supplementary cases | `crates/ferric-tools/src/builtin/check_syntax.rs::contextually_valid_control_flow_compiles`, `pep_695_alias_without_type_parameters_compiles`, `pep_695_type_parameters_are_preflighted_without_invoking_the_compiler`, `controlled_compiler_treats_invalid_utf8_as_invalid_source`, `controlled_compiler_bounds_source_size_without_starting_a_process` | Valid forms, nested generic guard, invalid UTF-8 and bounded unchecked input complement the narrower named matrix. The guard asserts its compiler closure was not invoked. In-process source ownership complements the no-file assertion; there is no separate process-spawn spy. PY, W-WIN, W-LINUX. |
| E01-B; AC-2 | `crates/ferric-tools/src/builtin/check_syntax.rs::except_star_is_valid`; `crates/ferric-tools/tests/controlled_mutations.rs::controlled_mutation_python_05_transition_matrix`, `python_syntax_matrix_blocks_regressions_and_warns_on_invalid_repairs` | Except-star is valid with no warning. Controlled rejection preserves prior bytes and absent paths; unchecked-to-invalid rejects, while repair and valid-to-valid distinctions remain. The two mutation tests compose the transition coverage. PY, M, W-WIN, W-LINUX. |

E01-A also executes
`crates/ferric-tools/tests/builtin_file_tools.rs::legacy_python_warning_is_in_process_and_does_not_execute_sitecustomize`
under W-WIN/W-LINUX: the customization marker remains absent and malformed
Legacy publication stays warning-only (INT-0005 AC-4/5). This specific
adversarial regression complements the in-memory RustPython call path; it is
not a universal process census.

## E02: Configuration and selected workspace

Affected intent: [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md)
AC-5/6; [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
partial AC-12. Valid API reload timing and direct-library policy admission are
not changed or newly accepted by these clauses.

| Clause / criteria | Exact source assertions | What the assertions establish / invocation keys |
|---|---|---|
| E02-A; INT-0006 AC-5/6 | `crates/ferric-cli/src/config.rs::config_absence_and_precedence`, `config_errors_redact_credentials`, `present_invalid_layer_is_bounded_and_never_overridden`, `invalid_effective_numbers_rejected`, `malformed_layer_never_exposes_user_defaults` | Absent/default precedence, tolerated unknown fields, sanitized Display/Debug and injected IO errors, invalid UTF-8, regular-file/size bounds and valid/invalid numeric boundaries. No invalid lower layer becomes silently overridden. W-WIN, W-LINUX, N. |
| E02-A consumer admission | `crates/ferric-cli/tests/cli.rs::present_invalid_config_blocks_all_consumers`, `invalid_effective_numbers_rejected_by_cli_surfaces`; `crates/ferric-cli/src/api.rs::present_invalid_config_blocks_all_consumers_api_reload`; `crates/ferric-cli/tests/human_cli.rs::human_invalid_config_blocks_before_preparation` | Legacy query/chat/MCP/skills/API malformed/enum/numeric/directory matrix, ICM invalid admission, effective-number benchmark checks, API HTTP error classification, and new run/status/explain rejection before lock/state preparation. Diagnostic credentials are excluded and prior config bytes retained. The human assertion passed under H and W-WIN. CLI, H, W-WIN, W-LINUX, applicable N cases. |
| E02-B; INT-0006 AC-6 | `crates/ferric-cli/src/backend.rs::selected_workspace_drives_real_provider`, `explicit_endpoint_and_ambiguous_discovery_matrix`; `crates/ferric-cli/tests/cli.rs::selected_workspace_drives_real_provider_chat_icm` | Selected B drives actual chat/ICM admission despite invocation A; explicit endpoint bypasses discovery; stale/conflict/unverifiable states refuse. Malformed registrations deliberately prevent network contact, so this is admission/discovery evidence rather than positive model communication from B. CLI, W-WIN, W-LINUX. |
| E02-C; INT-0006 AC-5/6 | `crates/ferric-cli/src/chat.rs::chat_effective_stream_matrix`; `crates/ferric-cli/tests/cli.rs::omitted_resume_harness_inherits` | A recording provider observes talk and escalation for six saved-stream/CLI combinations. Actual resumed traces retain both Legacy and Evidence when omitted. CLI, W-WIN, W-LINUX; resume also runs under N. |

The consumer tests are not a full Cartesian product of every invalid input and
surface. No-provider/no-hook effects also rely on admission ordering rather
than a dedicated invocation spy at every consumer. This limit does not license
silent fallback or a claim that an omitted surface was independently observed.

## E03: Foreground preparation and ownership

Affected intent: [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md),
prepared-host portions of AC-3/5/6/7/10/12. S and W-WIN exercise native Windows;
positive Linux identity fixtures are accepted only through W-LINUX.

| Clause / criteria | Exact source assertions | What the assertions establish / invocation keys |
|---|---|---|
| E03-A; AC-3/6/7/12 | `crates/ferric-cli/src/startup/tests.rs::startup_borrows_ready_server`, `startup_ambiguous_registration_is_nonmutating`, `borrowed_server_survives_session_exit`; `crates/ferric-cli/src/startup/probe_tests.rs::startup_credentials_stay_endpoint_bound` | Ready and explicit borrowing, unchanged registration, borrowed liveness after cleanup/Drop, explicit key retention and observed explicit Authorization header. S, W-WIN, W-LINUX. |
| E03-A additional refusal matrix | `crates/ferric-cli/src/startup/tests.rs::startup_typed_refusal_matrix_preserves_resources` | New source assertion constructs actual stale/conflict/degraded/unverifiable classifications, verifies actionable refusal, zero launch count, unchanged local/global records, no preference, surviving existing ownership and subsequent workspace admission. Passed under S and W-WIN. S, W-WIN, W-LINUX. |
| E03-B; AC-6 | `crates/ferric-cli/src/startup/tests.rs::startup_owned_runtime_reaches_ready`, `startup_listener_identity_mismatch_refuses` | Actual advertised model, context 4096, retained ownership, ready validation, no detached runfile, trace binding and checked exit. A foreign listener is refused and remains usable. Fixture command injection means the native closed engine argv/no-window/stdin details additionally need production-source and L/TTY evidence. S, W-WIN, W-LINUX; L/TTY for real launch. |
| E03-C; AC-6/7 | `crates/ferric-cli/src/startup/tests.rs::startup_cleanup_fault_matrix`, `startup_cancellation_reaps_scope`, `borrowed_server_survives_session_exit`, `startup_listener_identity_mismatch_refuses`; `crates/ferric-cli/src/human_journey_tests.rs::human_cancel_during_request_reaps_owned_engine` | Early exit, malformed metadata, unwind, bounded readiness cancellation, retained exact exit and unaffected borrowed/foreign resources. Human cancellation composes request failure with session cleanup and workspace reacquisition. S, HU, W-WIN, W-LINUX. |
| E03-C full preparation addition | `crates/ferric-cli/src/startup/tests.rs::startup_prepare_cancellation_automatically_reaps_and_unlocks` | New absent/listening-loading fixtures cancel the full preparation path; a retained observer does not signal the child, verifies its exit, and asserts no preference plus released listener/lock. Unlike the original readiness helper test, cleanup is automatic through preparation ownership. Passed under S and W-WIN. S, W-WIN, W-LINUX. |
| E03-D; AC-3/10 | `crates/ferric-cli/src/startup/tests.rs::startup_concurrent_invocations_serialize`; `crates/ferric-cli/src/startup/storage.rs::startup_concurrent_invocations_serialize`, `preferences_atomic_and_symlink_safe`; `crates/ferric-cli/src/startup/tests.rs::stale_preferences_do_not_reuse_qualification` | Two distinct concurrency functions: storage tests persistent lock identity; the new startup test uses bounded simultaneous rendezvous, retains the winner until both attempts resolve, asserts exactly one admission/launch, checked engine exit, preference reuse and unchanged expert-config sentinel. Staged interruption preserves prior preference; symlink replacement cannot overwrite the outside sentinel; stale GGUF requires reselection. Strengthened barrier passed under S and W-WIN. S, W-WIN, W-LINUX. |
| E03-D supplementary publication/binding cases | `crates/ferric-cli/src/startup/storage.rs::preference_replacement_after_publish_is_refused_without_cleanup_of_replacement`, `changed_staging_bytes_leave_the_prior_preference_readable`, `preference_changes_and_hardlinks_are_refused`, `malformed_oversized_and_authority_bearing_choices_are_refused`, `startup_state_directory_cannot_be_replaced_during_a_session`, `replaced_root_lock_cannot_admit_a_second_startup` | Postpublication replacement, staging tamper, hardlinks, malformed/oversized/authority-bearing preferences and directory/root-lock replacement remain separately asserted. Unix-specific checks are not inferred from Windows execution. S, W-WIN, W-LINUX as platform gated. |
| E03-E; AC-6/10/12 | `crates/ferric-cli/src/startup/tests.rs::startup_probe_limits_matrix`, `metadata_does_not_promote_authority`, `startup_bounded_version_probe`; `crates/ferric-cli/src/startup/probe_tests.rs::startup_probe_limits_streamed_bodies_and_redirects`, `startup_probe_cancellation_closes_headers_and_body`, `startup_probe_deadlines_are_finite` | Malformed/empty/oversized model metadata, counts, unsafe endpoints, cancellation closure, five-second/remaining-budget probes and bounded version attempts. Strengthened HTTP assertions now require the one-MiB-specific error for declared/chunked overflow and redirect-specific refusal with zero redirect-trap connections. Advertised context/qualification does not promote tool authority. Strengthened probes passed under S and W-WIN. S, W-WIN, W-LINUX. |
| E03-E exact bounds additions | `crates/ferric-cli/src/startup/tests.rs::startup_expired_180_second_budget_precedes_ready_effects`; `crates/ferric-cli/src/startup/models.rs::local_directory_entry_and_model_count_limits_are_exact` | New tests assert the 180-second constant and expired deadline before any request to an endpoint trap; exact 256 directory-entry and 128 GGUF admission boundaries and cap-specific errors. These do not wait 180 seconds. Passed under S and W-WIN. S, W-WIN, W-LINUX. |
| E03-F; AC-5 | `crates/ferric-cli/src/startup/tests.rs::startup_explain_is_read_only`; `crates/ferric-cli/tests/human_cli.rs::human_explain_does_not_contact_endpoint_or_prepare` | Local choices/unqualified context/no key/no new state; new process-level status/explain test observes no connection at the configured endpoint, checks borrowed ownership/effects/qualification JSON, excludes credentials and asserts no lock/state preparation. Passed under H and W-WIN. S, H, W-WIN, W-LINUX. |

Fixture-integrity supplements are
`crates/ferric-cli/src/startup/tests.rs::startup_fixture_keeps_fragmented_headers_bounded`
and
`crates/ferric-cli/src/human_journey_tests.rs::fixture_request_poll_timeout_preserves_fragments_and_absolute_bound`.
They preserve incomplete requests across polling timeouts while enforcing an
absolute bound; the startup test also verifies an actual fragmented health
request and stalled-request closure. These additions support E03/E06 fixture
validity, not a new product capability. Both passed under W-WIN and their S/HU filters.

`crates/ferric-cli/src/startup/probe_tests.rs::startup_fixture_write_preserves_partial_progress_and_deadline`
additionally injects transient/partial writes, perpetual backpressure and a late
final write. Its assertions preserve exact response bytes and reject deadline
overruns, supporting the specific streamed one-MiB admission proof.

Remaining assertion boundaries: implicit-discovery credential non-leak is not
independently wire-spied in the startup tests; explicit credential and prepared
generation transport tests complement source review. The new explain trap
covers the configured endpoint, not a universal process/download census. Shared
ProcessTree regression suites supply descendant cleanup guarantees beyond the
leader-focused startup fixtures. No hardware-fit or cross-workspace resource
exclusivity claim follows from these results.

## E04: Ordinary human entry point

Affected intent: [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md),
the AC-1/6/9/10/11/12 prepared-host portions specified by the locked plan.

| Clause / criteria | Exact source assertions | What the assertions establish / invocation keys |
|---|---|---|
| E04-A; AC-1/12 | `crates/ferric-cli/src/human_journey_tests.rs::human_first_run_decision_budget`; `crates/ferric-cli/tests/human_cli.rs::cargo_default_launch_selects_real_cli` | Shared session orchestration reaches an actual fixture answer with exactly three setup decisions; representative technical questions are absent; manifests select the CLI/default binary/backend. Scripted IO is not a native terminal. HU, H, W-WIN, W-LINUX; TTY supplies actual terminal evidence. |
| E04-B; AC-12 | `crates/ferric-cli/tests/human_cli.rs::no_args_non_tty_welcome_is_nonmutating`, `malformed_explicit_commands_remain_usage_errors` | No arguments and `run` produce at most twelve useful lines, exit zero and leave the workspace empty; malformed config is not opened by welcome. Explicit malformed commands/options and flags-only advanced return 2. Structural early return complements filesystem assertions; there is no separate zero-process census. H, W-WIN, W-LINUX, N. |
| E04-C; AC-6/12 | `crates/ferric-cli/src/human.rs::human_ask_never_dispatches`, `human_work_requires_scoped_consent`, `human_text_is_not_shell`, `new_session_does_not_inherit_edit_consent`, `human_work_policy_does_not_reuse_qualification_or_expert_authority`; `crates/ferric-cli/src/human_journey_tests.rs::human_journey_e2e_matrix` | Malicious tool-bearing Ask output cannot write; authority comes only from this session; shell-like input remains text. Full Work creates the expected file and records Evidence/conservative/write-file provenance. Policy assertions reject expert promotion and hooks/lineage/system prompt inheritance. HU, W-WIN, W-LINUX. |
| E04-D; AC-10/12 | `crates/ferric-cli/src/human_journey_tests.rs::human_repeat_reuses_model`, `human_stale_single_model_requires_reselection`, `human_journey_e2e_matrix`; `crates/ferric-cli/src/human.rs::human_decline_eof_and_errors_are_bounded`, `human_failure_is_concise` | Repeat removes the model-choice question; stale single-model reuse asks again; decline/EOF produce no engine/preference; representative diagnostic avalanche becomes one actionable line. This is not exhaustive copy coverage for every possible error. HU, W-WIN, W-LINUX. |
| E04-E; AC-1/11 | `crates/ferric-cli/tests/human_cli.rs::primary_help_is_compact`, `advanced_original_commands_compatible`, `no_default_features_welcome_and_mock_compatibility`; `crates/ferric-cli/src/main.rs::advanced_preserves_both_verbosity_positions`, `advanced_flags_alone_never_enter_human_session` | Four primary actions, direct/advanced help equivalence including API when enabled, actual advanced mock query, backend-free welcome/mock behavior and routing regressions. The API help-loop addition passed under H and W-WIN. Existing API wire assertions, including `crates/ferric-cli/src/api.rs::legacy_prompt_request_keeps_its_wire_shape`, supplement parser compatibility. H, W-WIN, W-LINUX, N. |

E04-D / INT-0008 AC-12 correction adds actual failure production through the
shared human renderer: `crates/ferric-cli/src/startup/probe_tests.rs::startup_probe_deadlines_are_finite`
and `human_real_metadata_failure_has_one_safe_action`, plus
`crates/ferric-cli/tests/human_cli.rs::human_read_only_admission_failure_has_one_safe_action`.
They retain real timeout/metadata/GGUF causes, exactly one safe action, bounded
diagnostic suppression, endpoint/cancellation compatibility and selected-B
read-only nonmutation. S/H passed at the corrected exact head; see the invocation table.

## E05: Provider cancellation and byte-correct streaming

Affected intent: [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
AC-6/12, named provider phases only. T-12024's existing Git snapshot boundary
prevents any universal two-second Work cancellation claim.

| Clause / criteria | Exact source assertions | What the assertions establish / invocation keys |
|---|---|---|
| E05-A; AC-6/12 | `crates/ferric-provider/src/openai_io_tests.rs::provider_cancellation_all_response_phases`, `cancelled_provider_does_not_poll_request`; `crates/ferric-cli/src/human_journey_tests.rs::human_cancel_during_request_reaps_owned_engine` | Six header/error-body/JSON/SSE cases assert Interrupted, connection closure and cancellation latency below two seconds; joined finite futures leave no server task detached. Human session cancellation then checks owned closure and workspace reacquisition. P, HU, W-WIN, W-LINUX. |
| E05-B; AC-12 | `crates/ferric-provider/src/openai_io_tests.rs::sse_unicode_every_split`, `sse_malformed_utf8_reports_error`, `sse_ascii_done_compatibility`, `sse_unicode_and_invalid_bytes_over_tcp` | Exact equality at every byte split and one-byte chunks for prose/native tool JSON/constrained content; invalid/incomplete UTF-8 errors; ASCII, usage, truncation and DONE compatibility; joined real TCP behavior. P, W-WIN, W-LINUX. |

The prepared transport additionally has
`crates/ferric-provider/src/openai_io_tests.rs::prepared_endpoint_keeps_loopback_direct_and_refuses_redirects`
and `prepared_endpoint_rejects_embedded_credentials`: injected proxy/redirect
traps must not receive prepared prompts or credentials. This complements E03-A
and does not replace the separate startup-probe credential boundary.

## E06: Composed journey and final qualification

Affected intents: [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
AC-9/11/12 and the explicitly affected INT-0005/INT-0006 criteria above.

| Clause / criteria | Exact source assertions | Composition and acceptance boundary / invocation keys |
|---|---|---|
| E06-A; INT-0008 AC-9/12 | `crates/ferric-cli/src/human_journey_tests.rs::human_journey_e2e_matrix`, `human_first_run_decision_budget`, `human_repeat_reuses_model`, `human_stale_single_model_requires_reselection`, `human_cancel_during_request_reaps_owned_engine`; E02 consumer assertions; `crates/ferric-cli/src/startup/tests.rs::startup_typed_refusal_matrix_preserves_resources`, `startup_concurrent_invocations_serialize`, `startup_prepare_cancellation_automatically_reaps_and_unlocks`, `startup_cleanup_fault_matrix`, `borrowed_server_survives_session_exit` | The locked plan explicitly permits composition across E02-E05. The named human matrix covers decline/EOF, absent model and successful owned Work; the listed startup/admission tests supply refusal, concurrency, startup cancellation, borrowed survival and fault cleanup. HU, S, CLI, H, W-WIN, W-LINUX. All constituent assertions passed under W-WIN; one function is not claimed to repeat every boundary. |
| E06-B; affected INT-0005/6/8 | `crates/ferric-cli/tests/source_execution.rs::source_quality_and_feature_matrix`, `source_driven_ci_contract`; `crates/ferric-cli/tests/human_docs.rs::first_run_docs_match_cli`; `crates/ferric-cli/tests/template_hygiene.rs::tracked_sources_carry_no_machine_identity`, `canonical_book_layout_replaces_legacy_live_ledgers`, `each_rule_rejects_identity_and_accepts_the_documentation_value`; existing CLI regressions | First command is `cargo r`; no README sprint heading; actual help/options/manifests match; workspace/no-default/platform CI and source-ownership ratchets remain. The README-heading addition passed under H and W-WIN. Deferred findings remain visible in `docs/work/tasks.md`. H, CLI, F, C, CN, N, W-WIN, W-LINUX, LF-WIN, LF-LINUX and remaining required CI. Static workflow assertions cannot prove CI passed. |
| E06-C; INT-0008 AC-6/9/12 partial | `crates/ferric-cli/src/human_journey_tests.rs::real_model_prepared_host_journey`; actual Cargo PTY transcript | L is ignored in ordinary suites and must run explicitly. It records actual model/runtime/settings, decisions, timings, transcript/trace/result; asserts answered trace, owned cleanup outcome and workspace reacquisition. Final-head L and TTY passed with retained E2E evidence. Earlier Build trials remain separately bound. No acquisition, hardware qualification, full workflow resume, application build or medium-horizon success is inferred. |

## Closure requirements

- Bind the final implementation commit and exact commands/results to every
  clause, including the new human-invalid/explain, API help, README-heading,
  startup barrier/refusal/full-prepare cancellation and exact-bound assertions.
- Retain all failed attempts and cleanup outcomes. Report a live failure or
  unsupported native identity environment honestly; do not substitute a static
  ratchet, compile check, non-TTY welcome or aggregate count.
- Require native Windows and isolated native Linux CI results plus the existing
  backend-free, lifecycle and aarch64 gates at the exact implementation head.
- Rerun and retain L/TTY evidence at the final head as required by the Test
  record; link it without promoting earlier Build results to current acceptance.
- Keep INT-0007/INT-0008 acquisition, calibration, complete resume/status,
  hardware parity and application qualification follow-ups visible. Keep
  T-12024's Git cancellation limitation explicit.
- Complete independent Test critique and later workflow gates before marking
  acceptance; this document alone authorizes no success claim or PR offer.
