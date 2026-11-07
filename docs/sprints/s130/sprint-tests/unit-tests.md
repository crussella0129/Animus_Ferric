# Sprint 130 — Unit tests

New unit tests guard the refusal and compatibility paths that the
retirements left behind:

- `capture_reports_retired_records_for_manual_cleanup`: seven retired
  record shapes across three scopes.
- `retired_registrations_block_status_down_and_doctor_without_effects`.
- `single_binary_parser_keeps_only_the_legacy_child_policy`.
- `retained_trace_name_keeps_the_single_legacy_layout`.
- `pre_retirement_paired_and_single_rows_still_parse`.
- `checkpoint_controller_block_counts_must_match_typed_events`, renamed from
  the omitted-count test.

Every other removed unit test belonged to the retired feature.
[The test report](test-report.md) lists the guards per retirement.
