# Sprint 113 Unit Test Results

- **Tested code head:** `dbaada383cd58415dfc775ec2c9d7e55a28bbcd0`
- **Executed:** 2026-08-26
- **Result:** pass
- **Intent oracle:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md), acceptance criteria 1–8

## Canonical suite confirmation

`cargo test -p ferric-core -p ferric-trace -p ferric-tools -p ferric-loop
-p ferric-bench -p animus-launch` exited zero. It included 32 core tests, 32
trace tests, 131 loop library tests, 172 tools tests, 79 passing bench tests
with two declared child-process fixtures ignored, and 22 Animus Launch tests.
All crate and doc-test targets completed with zero failures.

The following named results bind every model-free EARS promise to its SHALL
assertion. Names not repeated here remain covered by the locked test plan and
the full passing package targets.

## EARS evidence

| Clause | Arrangement and SHALL assertion | Executed named evidence |
| --- | --- | --- |
| T-11301.1 | A literal pre-Evidence trace/policy line defaults to Legacy and invents no controller state. | `pre_evidence_trace_fixture_replays_as_legacy`; `an_old_policy_line_reads_back_at_the_default_cap_and_legacy_harness` |
| T-11301.2 | Controller events are accepted only under Evidence, at causal positions, with supported versions. | `legacy_policy_rejects_known_controller_events_instead_of_ignoring_them`; `evidence_policy_accepts_a_causally_matched_observation`; `evidence_event_versions_and_order_fail_closed` |
| T-11302.1 | Successful navigation exposes explicit normalized ranges/completeness and stable content identity, including zero results. | `read_file_preserves_exact_crlf_range_and_hashes_complete_raw_file`; `navigation_zero_results_are_explicit_and_stably_hashed`; `truncated_read_is_honest_and_retains_full_trace_output` |
| T-11302.2 | Stale, identical, opaque, and invalid syntax candidates refuse before approval/publication. | `cas_race_returns_typed_expected_and_observed_identities_without_effects`; `identity_missing_match_and_net_zero_candidates_are_typed_no_effects`; `controlled_write_fails_closed_without_typed_preparation`; `python_syntax_matrix_blocks_regressions_and_warns_on_invalid_repairs` |
| T-11302.3 | Real content and structural calls report measured path effects. | `all_content_tools_prepare_then_commit_exact_candidates_after_a_clean_reset`; `create_and_modify_effects_preserve_exact_crlf_bytes_and_line_counts`; complete `controlled_structural` target |
| T-11303.1 | Blind and same-turn existing-content mutations stop before callback/commit. | `prior_turn_boundary_and_complete_reread_are_enforced`; `same_turn_observation_cannot_authorize_a_later_call_in_the_batch` |
| T-11303.2 | Failed checks require later qualifying inspection; unchanged checks do not spawn again. | `failed_check_requires_a_later_turn_global_barrier_and_path_specific_read`; `failed_check_repair_rebuilds_same_identity_coverage_from_later_pages`; `same_named_check_at_same_epoch_is_refused_before_a_second_attempt` |
| T-11303.3 | One call with one or many real effects advances exactly one epoch and shared verification agrees. | `measured_effect_advances_once_and_records_exact_authored_evidence`; `multi_path_effect_advances_one_epoch_and_trace_verifies` |
| T-11304.1 | Pause/crash replay stales inherited coverage, restores controller truth, and emits canonical bytes without invented effects. | `evidence_replay_preserves_a_complete_intentional_pause_suffix`; `evidence_crash_prefix_retries_predispatch_without_inventing_an_effect`; `checkpoint_and_recovery_packet_are_canonical_and_byte_stable`; `resume_discards_inherited_coverage_and_rebuilds_it_from_new_pages` |
| T-11304.2 | Resume-of-resume and compaction cannot replace projected controller truth with model prose. | `evidence_resume_of_resume_uses_the_latest_projected_controller_state`; `history_compaction_does_not_change_projected_controller_truth`; `evidence_clarification_resume_anchors_the_answer_without_a_generic_packet`; `evidence_resume_packet_is_literal_history_after_base_or_generic_anchors` |
| T-11306.1 | Frozen arms are distinct files, copied read-only, adjacent/counterbalanced, fresh, and collision-safe. | `frozen_pair_rejects_nonfiles_and_equal_digests`; `frozen_pair_uses_verified_read_only_copies`; `paired_schedule_is_adjacent_deterministic_and_counterbalanced`; `retained_trace_names_cannot_collide_across_policy_arms`; `paired_freshness_requires_distinct_instances_and_equal_initial_trees` |
| T-11306.2 | Missing, dirty, invalid, or unpaired coordinates never become model losses. | `paired_summary_is_coordinate_complete_and_uses_only_eligible_pairs`; `invalid_trace_or_unpaired_rows_are_excluded_not_scored_as_losses`; `strict_rows_fail_closed_on_trace_or_managed_server_tampering` |
| T-11309.1 | Model-authored Python is parsed in-process, bounded, without importing workspace code. | `legacy_python_warning_is_in_process_and_does_not_execute_sitecustomize`; `controlled_compiler_bounds_source_size_without_starting_a_process`; `candidate_compiler_creates_no_temp_or_pycache_files` |
| T-11309.2 | Invalid Legacy Python remains warning-only and its exact bytes are published. | `broken_python_returns_in_process_warning`; invalid-source branch of `legacy_python_warning_is_in_process_and_does_not_execute_sitecustomize` |
| T-11309.3 | A valid `sitecustomize.py` cannot create its import-time marker. | `legacy_python_warning_is_in_process_and_does_not_execute_sitecustomize` |

## Negative-path assessment

The executed matrix includes unsupported versions, causal misordering, Legacy
controller events, blind/same-turn edits, CAS races, identical candidates,
opaque tools, invalid syntax transitions, repeated checks, stale recovery
coverage, dirty pairs, equal binaries, malformed provenance, and tampered
traces. One narrow proof limitation remains: the Python boundary has a direct
no-side-effect/import test and source contains no subprocess path, but no fake
`PATH` process-spawn canary. This does not contradict the passing in-process
implementation; it is retained for the critic rather than silently upgraded to
a stronger claim.
