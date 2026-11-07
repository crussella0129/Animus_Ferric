# Sprint 129 — Unit tests

Sprint 129 is a refactor, so most unit tests carried over unchanged and act
as its equivalence proof. The [test report](test-report.md) lists every
removed test with the test that still covers it.

New unit tests:

- **Regression tests for the defects:**
  - D-2: `only_listing_forms_of_branch_and_no_output_files_are_read_only`;
  - D-4: `stream_error_event_is_a_backend_error`;
  - D-7: `startup_explain_lists_the_resolved_models_folder`;
  - CI zombie fix:
    `a_zombie_holds_no_sockets_and_does_not_block_owner_inspection`.
- **Equivalence pins:**
  - `mechanism_counts_serialize_byte_identically` (bench JSON, captured
    from the pre-refactor code);
  - `status_registration_lines_are_golden_for_every_runtime_observation`
    (replaces a copied renderer used as its own oracle);
  - `longest_prefix_stops_at_the_first_failed_or_missing_level`;
  - `jsonl_reader_names_a_malformed_line`.
- **Moved without change:** the former integration tests of animus-launch,
  ferric-icm and ferric-vcs now run as unit tests (`launch::scaffold_tests`,
  `icm::workspace_tests`) or as ferric-loop integration tests
  (`vcs_tests`), under their original names.
