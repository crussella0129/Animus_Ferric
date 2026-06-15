# Sprint 2 Unit Tests — Results

Run 2026-06-14, local Windows (rustc 1.93.1), default features: **ALL PASS** (112 default-feature tests workspace-wide; feature-gated tests run separately under `--features backend-mistralrs`).

| Task | EARS clause → test | Result |
|---|---|---|
| T-201 | preserve_order active → `ferric_bench::tests::preserve_order_active` | pass |
| T-201 | default graph mistralrs/tokio-free → `cargo tree` (verified) | pass |
| T-202 | per-tier max_output_tokens → `tier_table_snapshot` (extended) + `max_output_tokens_per_tier` | pass |
| T-202 | ActionProtocol serde → `action_protocol_serde` | pass |
| T-203 | new events round-trip → `jsonl_roundtrip_all_event_types` (extended: PolicySelected/PromptComposed) | pass |
| T-204 | finish_reason mapping → `finish_reason_maps_truncated` (feature-gated) | pass |
| T-205 | move file/dir → `move_path_renames_file`, `_dir` | pass |
| T-205 | missing source error → `move_path_missing_source_is_error` | pass |
| T-205 | cross-boundary/.ferric deny → `move_path_outside_to_denied`, `move_path_into_ferric_denied` | pass |
| T-205 | make_dir parents+idempotent → `make_dir_creates_parents_and_is_idempotent` | pass |
| T-206 | schema golden (anyOf, no oneOf, ap:false) → `schema_golden` | pass |
| T-206 | tool-before-args order → `tool_precedes_args_in_serialization` | pass |
| T-206 | deterministic branch order → `branches_in_offered_order_terminator_last` | pass |
| T-206 | parse_action round-trip + rejections → `parse_action_roundtrip`, `_rejects_garbage`, `_partial_json`, `_non_action_object` | pass |
| T-207 | select_protocol matrix → `select_protocol_matrix` | pass |
| T-209 | compose all tier×protocol + lineage → `compose_all_pairs` | pass |
| T-209 | protocol-exclusive teaching → `protocol_teaching_is_exclusive` | pass |
| T-209 | typed errors → `missing_library_is_typed_err`, `version_mismatch_is_typed_err` | pass |
| T-211 | specs parse + unknown-field reject → `all_seven_specs_parse`, `unknown_field_rejected` | pass |
| T-211 | Ferric tool names L0/L1/L2 → `l0_forbids_mutations_and_uses_ferric_tool_names`, `l1_l2_use_ferric_mutation_tools` | pass |
| T-213 | verdict matrix → `tools_verdict_matrix`, `completed_truth_table` | pass |
| T-213 | failure admission → `failure_admission_detects_phrases` | pass |
| T-213 | append-not-truncate → `append_is_not_truncate` | pass |
| T-214 | measured_level selection → `highest_completed_is_measured_level` | pass |
| T-214 | bidirectional override (1B+L4→Small) → `calibrate_records_both_tiers` | pass |
| T-214 | profile replace-key/keep-others → `write_profile_replaces_same_key_keeps_others` | pass |
| T-214 | failed L0 → no measured_level → `failed_l0_has_no_measured_level` | pass |

Feature-gated (`--features backend-mistralrs`): T-204 + s1 mapping suite all pass; CI's backend-check compiles them under `-D warnings`.
