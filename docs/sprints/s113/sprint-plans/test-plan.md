Finalized - DO NOT EDIT

# Sprint 113 Test Plan

## Intent Traceability

| Intent | Acceptance criterion | Build task / EARS clause | Verification |
| --- | --- | --- | --- |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-1 typed evidence and legacy compatibility | T-11301 / pre-evidence data defaults to legacy | `pre_evidence_trace_fixture_replays_as_legacy`; `an_old_policy_line_reads_back_at_the_default_cap_and_legacy_harness` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-1 central causal validation | T-11301 / controller events accepted only when versioned and causally placed | `legacy_policy_rejects_known_controller_events_instead_of_ignoring_them`; `evidence_policy_accepts_a_causally_matched_observation`; `evidence_event_versions_and_order_fail_closed` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-2 observations and safe publication | T-11302 / explicit envelope, typed stale/no-effect/syntax/effect outcomes | `read_file_preserves_exact_crlf_range_and_hashes_complete_raw_file`; `navigation_zero_results_are_explicit_and_stably_hashed`; `identity_missing_match_and_net_zero_candidates_are_typed_no_effects`; `python_syntax_matrix_blocks_regressions_and_warns_on_invalid_repairs`; `controlled_structural` suite |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-2/3 causal controller | T-11303 / blind or same-turn mutation blocks; one epoch per effect; repair barrier and unchanged-check refusal | `blind_and_same_turn_mutations_are_rejected_before_the_callback`; `measured_effect_advances_once_and_records_exact_authored_evidence`; `full_evidence_path_reads_edits_repairs_verifies_and_completes`; `repeated_check_is_blocked_before_a_second_process_starts` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-4 durable recovery | T-11304 / pause/crash replay, evidence staling, packet and compaction invariants | `evidence_crash_prefix_retries_predispatch_without_inventing_an_effect`; `resume_discards_inherited_coverage_and_rebuilds_it_from_new_pages`; `evidence_resume_of_resume_uses_the_latest_projected_controller_state`; `evidence_replay_preserves_a_complete_intentional_pause_suffix`; `history_compaction_does_not_change_projected_controller_truth` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-5 product compatibility and fail-closed planner | T-11305 / dispatch order, shared policy, resume inheritance/mismatch, planner rejection | `full_evidence_path_reads_edits_repairs_verifies_and_completes`; `stale_commit_is_typed_and_an_admitted_call_prompts_once`; `sink_denial_happens_before_a_verification_process_starts`; `resume_target_inherits_omitted_policy_and_rejects_an_explicit_mismatch`; `evidence_talk_is_allowed_but_do_escalation_fails_explicitly`; `planner_rejection_writes_no_trace_or_workspace_effect` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-6 paired runner | T-11306 / frozen AB/BA arms and scoreable-row gate | `frozen_pair_rejects_nonfiles_and_equal_digests`; `frozen_pair_uses_verified_read_only_copies`; `paired_schedule_is_adjacent_deterministic_and_counterbalanced`; `retained_trace_names_cannot_collide_across_policy_arms`; `retained_trace_digest_and_validation_use_one_immutable_byte_snapshot`; `paired_summary_is_coordinate_complete_and_uses_only_eligible_pairs`; `invalid_trace_or_unpaired_rows_are_excluded_not_scored_as_losses` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-2 no implicit execution during syntax validation | T-11309 / in-process candidate parsing and warning-only compatibility | `legacy_python_warning_is_in_process_and_does_not_execute_sitecustomize`; `broken_python_returns_in_process_warning`; `python_syntax_matrix_blocks_regressions_and_warns_on_invalid_repairs` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-5 feature-gated surface propagation | T-11310 / unavailable planner preflight and bounded supported-Evidence propagation | `backend_surface_policy_propagation`; `unsupported_planner_fails_before_api_bind_or_trace`; backend-openai CLI test target |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-7 development screen and revision cap | T-11307 / objective, safety, mechanism, and clarification selection gates; retained first screen; at most two attributed 0/3 revisions; no held-task inspection | `real_model_evidence_screen`; `screen_revision_budget_and_provenance_audit` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-7 frozen paired confirmation | T-11311 / frozen hash and 18 clean rows, or explicit no-candidate skip | `paired_qwen_confirmation`; `frozen_confirmation_binary_audit`; `falsified_candidate_skip_audit` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-7 held-task generalization and teardown | T-11312 / measurable held promotion, untouched or sealed coordinates, verified traces, independent shutdown | `held_task_comparison`; `held_promotion_verdict_audit`; `retained_trace_verification_audit`; `managed_server_teardown` |
| [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) | AC-8 explicit planner decision and coherent close | T-11308 / decision artifact links measured evidence and no silent fallback | `evidence_planner_fails_closed_until_its_trace_protocol_exists`; `planner_decision_evidence_audit`; `book_close_evidence_audit` |

## Unit Tests

### T-11301 wire tests
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `pre_evidence_trace_fixture_replays_as_legacy`: a literal pre-Sprint-113 trace reconstructs without controller facts.
- `an_old_policy_line_reads_back_at_the_default_cap_and_legacy_harness`: literal old wire input defaults to legacy.
- `legacy_policy_rejects_known_controller_events_instead_of_ignoring_them`: legacy cannot carry evidence-only facts.
- `evidence_policy_accepts_a_causally_matched_observation`: matched evidence sequence is valid.
- `evidence_event_versions_and_order_fail_closed`: unsupported versions and misplaced controller events fail shared validation.

### T-11302 tool tests
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `read_file_preserves_exact_crlf_range_and_hashes_complete_raw_file`: file bytes, range, and full SHA remain honest.
- `navigation_zero_results_are_explicit_and_stably_hashed`: empty literal search is explicit rather than ambiguous.
- `identity_missing_match_and_net_zero_candidates_are_typed_no_effects`: no-effect candidates never publish.
- `python_syntax_matrix_blocks_regressions_and_warns_on_invalid_repairs`: supported syntax admission follows the approved transition matrix.
- `controlled_structural` suite: create/delete/move/copy effects and stale preconditions are typed and measured.

### T-11303 controller tests
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `measured_effect_advances_once_and_records_exact_authored_evidence`: one real call advances one epoch.
- `failed_check_repair_rebuilds_same_identity_coverage_from_later_pages`: repair evidence must be later and complete.
- `same_turn_observation_cannot_authorize_a_later_call_in_the_batch`: native multi-call batching cannot evade the turn boundary.

### T-11304 recovery tests
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `checkpoint_and_recovery_packet_are_canonical_and_byte_stable`: controller packet rendering is deterministic.
- `evidence_resume_of_resume_uses_the_latest_projected_controller_state`: nested resumes use the latest valid state.
- `history_compaction_does_not_change_projected_controller_truth`: compaction cannot rewrite controller facts.

### T-11306 result-math tests
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `paired_summary_is_coordinate_complete_and_uses_only_eligible_pairs`: only complete clean pairs are scored.
- `invalid_trace_or_unpaired_rows_are_excluded_not_scored_as_losses`: infrastructure/trace defects are not model losses.
- `paired_freshness_requires_distinct_instances_and_equal_initial_trees`: arm workspaces are fresh and comparable.

### T-11309 syntax-boundary tests
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `legacy_python_warning_is_in_process_and_does_not_execute_sitecustomize`: a valid customization module with an import-time marker is written as bytes only; no marker appears and no interpreter is needed.
- `broken_python_returns_in_process_warning`: invalid source retains legacy warning-only behavior using the Rust parser.
- `python_syntax_matrix_blocks_regressions_and_warns_on_invalid_repairs`: evidence mode continues to enforce the approved pre-publication transition matrix through the same parser.

## Integration Tests

### Controlled loop integration
- **Intents:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `full_evidence_path_reads_edits_repairs_verifies_and_completes`: read → edit → failed check → later inspection → repair → pass → completion.
- `blind_and_same_turn_mutations_are_rejected_before_the_callback`: controller denial precedes approval.
- `repeated_check_is_blocked_before_a_second_process_starts`: unchanged recheck consumes no second process.
- `evidence_guidance_is_added_to_custom_prompts_and_legacy_is_literal`: policy guidance is scoped and legacy remains literal.
- `stale_commit_is_typed_and_an_admitted_call_prompts_once`: dispatch ordering and single approval hold around CAS.
- `sink_denial_happens_before_a_verification_process_starts`: rejection precedes process execution.
- `evidence_talk_is_allowed_but_do_escalation_fails_explicitly`: interactive chat's supported and unavailable policy boundaries are explicit.

### CLI and benchmark integration
- **Intents:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- `resume_target_inherits_omitted_policy_and_rejects_an_explicit_mismatch`: resume compatibility boundary.
- `evidence_runs_and_planner_fails_before_trace_or_workspace_mutation`: supported evidence and unavailable planner behavior.
- `paired_schedule_is_adjacent_deterministic_and_counterbalanced`: deterministic arm order.
- `paired_server_provenance_requires_known_single_slot_sampling_controls`: managed-server identity gate.
- `frozen_pair_rejects_nonfiles_and_equal_digests`: control and candidate identity must be distinct and regular.
- `frozen_pair_uses_verified_read_only_copies`: run-owned arm binaries match verified source hashes.
- `retained_trace_names_cannot_collide_across_policy_arms`: trace retention cannot overwrite another coordinate.
- `retained_trace_digest_and_validation_use_one_immutable_byte_snapshot`: persisted trace bytes, digest, and validation are one snapshot.
- `backend_surface_policy_propagation`: bounded API/MCP/ICM request seams record Evidence.
- `unsupported_planner_fails_before_api_bind_or_trace`: EvidencePlanner fails before bind/trace/mutation.
- Backend gate: `cargo test -p ferric-cli --features backend-openai` completes without a hanging server future.

## End-to-End Tests

- **Status:** possible
- `real_model_evidence_screen`: pinned Qwen/model hash, H01/H04/H08 recovery, one evidence row per task; selection requires at least one objective+contract completion, complete clean rows, verified traces, zero unsafe completions or admitted mechanism violations, and no more than one unnecessary clarification. A nonzero result failing those gates is falsified; only a 0/3 result can enter the bounded revision path.
- `paired_qwen_confirmation`: after candidate freeze, three adjacent counterbalanced repeats for each H01/H04/H08 coordinate; pass requires a positive paired objective delta and one task completed by evidence in at least two of three repeats without safety, contract, clarification, or mechanism regression. If no candidate qualifies, `falsified_candidate_skip_audit` proves no confirmation row ran.
- `held_task_comparison`: frozen arms on untouched H02/H03/H05/H06/H07; promotion requires a positive aggregate paired objective-completion delta, at least one evidence-only objective+contract pass, no loss of a control-passing contract, no clarification increase, and zero unsafe completions or mechanism violations. If confirmation was skipped, `held_promotion_verdict_audit` proves the held tasks remained sealed; otherwise it records pass or falsification without tuning.
- `managed_server_teardown`: runfile PID, executable, model, listener, health, and model endpoint match before shutdown; PID/listener/runfile/matching process are absent afterward.
- `book_close_evidence_audit`: intent, tasks, completion ledger, test report, critique, and sprint metadata all link the same outcome and preserve `evidence_planner`'s explicit availability state.
- `planner_decision_evidence_audit`: `planner-decision.md` exists, links measured and skipped evidence, records an explicit design or rejection, and explains the causal rationale.

The full Rust quality gates are `cargo fmt --check`, affected crate tests,
`cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`, and the
backend-openai CLI clippy/test configuration from the approved verification
contract.
- `evidence_crash_prefix_retries_predispatch_without_inventing_an_effect`: safe crash prefix behavior is explicit.
- `resume_discards_inherited_coverage_and_rebuilds_it_from_new_pages`: resumed file evidence cannot authorize mutation until reread.
