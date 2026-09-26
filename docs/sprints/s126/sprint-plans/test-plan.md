# Sprint 126 Test Plan

## Intent Traceability
| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|--------|----------------------|--------------------------|--------------|
| [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md) | AC-2 consumers use the core; suite unchanged | T-12601 / WHEN the workspace builds THEN pre-existing paths SHALL resolve | `cargo build --workspace --all-targets --locked` (compile gate) + `reexport_paths_resolve` |
| INT-0011 | AC-2 | T-12601 / WHEN the suite runs THEN totals SHALL equal the baseline | `suite_totals_unchanged` (recorded baseline vs post-move totals in `extraction-measurement.md`) |
| INT-0011 | AC-2 | T-12601 / WHEN the loop renders the prompt or a tool result THEN it SHALL call the core and bytes SHALL be unchanged | `tool_listing_bytes_match_legacy_format`, `tool_result_text_matches_legacy_format`, plus existing `ferric-loop` replay/projector tests |
| INT-0011 | AC-1 dependency boundary | T-12602 / WHEN manifests gain an out-of-set crate THEN the test SHALL fail naming it | `iron_dependencies_are_within_boundary`, `boundary_check_names_offending_crate` |
| INT-0011 | AC-1 | T-12602 / WHEN extraction lands THEN measurements SHALL be recorded | `extraction-measurement.md` review (artifact check in Test phase) |
| INT-0011 | AC-1 | T-12602 / WHEN the gate fails THEN Stage B SHALL NOT start | Build-phase gate record in `extraction-measurement.md` (ordering evidence: Stage B commits follow the gate commit) |
| INT-0011 | AC-4 no dropped constraint | T-12603 / WHEN a Regex constraint is sent THEN InvalidRequest before I/O | `regex_constraint_is_rejected_without_network`, `regex_constraint_is_rejected_when_streaming` |
| INT-0011 | AC-4 | T-12603 / WHEN other requests are built THEN bodies SHALL be byte-identical | existing `build_body_*` tests + `build_body_unchanged_for_supported_constraints` |
| INT-0011 | AC-3 adapter | T-12604 / WHEN given OpenAI tools THEN ordered descriptors with defaults | `openai_tools_adapt_in_order`, `openai_tools_default_missing_parameters` |
| INT-0011 | AC-3 adapter | T-12604 / WHEN an entry is invalid THEN a typed error naming it | `openai_tools_reject_non_function`, `openai_tools_reject_duplicate_name`, `openai_tools_reject_reserved_control_name`, `openai_tools_reject_missing_name` |
| INT-0011 | AC-3 adapter | T-12604 / WHEN descriptors reach `action_schema` THEN one branch per descriptor in order | `adapted_tools_schema_has_one_branch_each` |
| [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md) | AC-1 request contract | T-12605 / WHEN tools offered THEN constrained body per contract | `constrained_transform_rewrites_request`, `constrained_transform_inserts_system_when_absent`, `constrained_transform_forwards_unknown_fields` |
| INT-0012 | AC-1 / pass-through boundary | T-12605 / WHEN no tools, `tool_choice:none`, or `response_format` THEN pass-through byte-identical | `passthrough_when_no_tools`, `passthrough_when_tool_choice_none`, `passthrough_when_response_format` |
| INT-0012 | AC-1 history projection | T-12605 / WHEN history has tool calls THEN canonical action + tool_result text | `history_tool_call_projects_to_canonical_action`, `history_tool_message_projects_to_tool_result_text` |
| INT-0012 | AC-1 history projection | T-12605 / WHEN history has a final answer THEN canonical task_complete action | `history_final_answer_projects_to_task_complete` |
| INT-0012 | AC-1 explicit errors | T-12605 / WHEN an orphan tool_call_id or non-object arguments THEN typed error → 400 | `orphan_tool_call_id_is_rejected`, `non_object_arguments_are_rejected`, integration `server_maps_transform_error_to_400` |
| INT-0012 | Deterministic prompt bytes | T-12605 / WHEN transformed twice or extended THEN byte-identical / prefix-stable | `transform_is_deterministic`, `appended_turns_preserve_rendered_prefix` |
| INT-0012 | AC-1 response contract | T-12606 / WHEN action is an offered tool THEN one tool_call, reasoning_content, finish tool_calls | `translate_tool_action`, integration `stream_tool_call_end_to_end` |
| INT-0012 | AC-1 response contract | T-12606 / WHEN action is task_complete THEN content = summary, finish stop | `translate_final_answer`, integration `stream_final_answer_end_to_end` |
| INT-0012 | Honest failures | T-12606 / WHEN upstream finish is length THEN finish length, no partial action | `translate_length_is_not_parsed`, integration `length_truncation_is_reported` |
| INT-0012 | Honest failures | T-12606 / WHEN output unparsable or tool not offered THEN 502, never raw text | `translate_rejects_unparsable`, `translate_rejects_unoffered_tool`, integration `malformed_upstream_yields_502` |
| INT-0012 | Honest failures | T-12606 / WHEN upstream is non-2xx or unreachable THEN 502 with status, no retry | integration `upstream_http_error_yields_502_with_status`, `upstream_unreachable_yields_502` |
| INT-0012 | AC-2 streaming | T-12606 / WHEN stream:true THEN chunk order role→reasoning→name→args/content→finish→DONE; heartbeat only on upstream progress | integration `stream_chunk_order`, `heartbeat_only_on_upstream_progress`, `no_heartbeat_when_upstream_stalls` |
| INT-0012 | AC-2 non-stream equivalence | T-12606 / WHEN stream:false THEN same final message | integration `nonstream_equals_assembled_stream` |
| INT-0012 | AC-3 cancellation | T-12606 / WHEN client disconnects THEN upstream closes within bound | integration `client_disconnect_closes_upstream` |
| INT-0012 | AC-4 receipts | T-12607 / WHEN a request completes/fails/cancels THEN exactly one content-free receipt | `receipt_has_required_fields`, `receipt_marks_unavailable_metrics`, integration `one_receipt_per_request`, `receipt_contains_no_message_text`, `cancelled_request_writes_cancelled_receipt` |
| INT-0012 | AC-6 enforcement probe | T-12607 / WHEN started constrained THEN probe; exit non-zero unless `{"ok":"yes"}` | integration `probe_accepts_enforcing_upstream`, `probe_refuses_non_enforcing_upstream` |
| INT-0012 | Record-only boundary | T-12607 / WHEN --record-only THEN byte-for-byte forwarding + record-only receipts | integration `record_only_is_byte_faithful_streaming`, `record_only_is_byte_faithful_nonstream` |
| INT-0012 | Loopback default | T-12607 / WHEN --listen omitted THEN bind 127.0.0.1 | `default_listen_is_loopback` |
| INT-0012 | AC-5 conformance with Hermes shapes | T-12605–T-12607 | integration `hermes_captured_request_round_trips` (fixture captured from Amalgam request shape) |
| INT-0012 | AC-7 live readiness | T-12608 / lab refusal, resource gate, owned launch, derived deadlines, session isolation | E2E `hermes_pilot --plan smoke` run records + unit `lab_root_inside_repo_is_refused`, `deadline_is_derived_from_measured_rates`, `admission_refuses_unknown_or_insufficient_memory` |
| INT-0012 | AC-7 (enabling move) | T-12608 / WHEN the memory probe moves to ferric-process THEN CLI fit behavior and memory tests unchanged | moved `parse_meminfo`/probe tests pass in `ferric-process`; existing `ferric-cli` fit/picker tests pass unchanged |
| INT-0012 | AC-7 live readiness | T-12608 / WHEN 7B bring-up and 27B smoke run THEN a tool call + final answer + extending prefix hashes | E2E `bringup_7b_valve`, `readiness_27b_valve` (receipts + driver result) |
| INT-0012 | AC-3 live cancellation | T-12608 / WHEN interrupted THEN slot idle within bound, receipt cancelled | E2E `readiness_27b_cancel` |
| INT-0012 | Owned children | T-12608 / WHEN runner exits by any path THEN children reaped | E2E runner cleanup record for every run + `ProcessTree` existing tests |
| [INT-0013](../../../intents/INT-0013-constrained-decoding-on-midsize-quantized-target.md) | AC-3 paired pilot | T-12609 / WHEN pilot runs THEN 4×2×3 ABBA on one server | E2E `pilot_27b` session ledger in `pilot/report.md` |
| INT-0013 | Independent checking | T-12609 / WHEN a session ends THEN checker judges from state/answer | `checker_*` unit tests for each task + E2E ledger |
| INT-0013 | AC-3 denominators | T-12609 / WHEN report written THEN per-arm metrics incl. exclusions | `pilot/report.md` review against the required table |
| INT-0013 | AC-1 manifest | T-12609 / WHEN report written THEN manifest + no-claim statement | `pilot/manifest.json` + report review |
| [INT-0010](../../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md) | AC-1 docs | T-12610 / WHEN landing docs read THEN role, split, maintenance, envelope | `landing_docs_state_iron_role` (doc-content test) |
| INT-0010 | AC-3 ledger | T-12610 / WHEN tasks.md read THEN each open task is Iron-linked or under Maintenance; count not lower | `open_tasks_are_triaged`, `task_line_count_not_reduced` |
| INT-0010 | AC-5 instructions | T-12610 / WHEN CLAUDE.md/AGENTS.md read THEN direction section present | `project_instructions_state_direction` |
| INT-0010 | AC-6 green main | carried fix `bf3c7d8` | PR CI green on ubuntu + windows |

## Unit Tests

### T-12601 unit tests
- **Intent:** [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md)
- `reexport_paths_resolve`: a `ferric-loop` test that imports each moved item through the old `ferric_provider::` / `ferric_loop::` path and uses it (compile + trivial assertion).
- `tool_listing_bytes_match_legacy_format`: `render_tool_listing` over two tools + controls equals the literal pre-move `"\n\nAvailable tools:\n- a: …\n"` bytes.
- `tool_result_text_matches_legacy_format`: `tool_result_text("read_file","x")` == `"[tool_result for read_file] x"`.
- Moved tests (grammar, protocol, terminator, scanner) travel with their code unchanged.

### T-12602 unit tests
- **Intent:** INT-0011
- `iron_dependencies_are_within_boundary`: parses both manifests; asserts dependency keys ⊆ allowed sets.
- `boundary_check_names_offending_crate`: the checker function applied to a synthetic manifest containing `tokio` returns an error naming `tokio` and the manifest.

### T-12603 unit tests
- **Intent:** INT-0011
- `regex_constraint_is_rejected_without_network`: `complete` against an unroutable base URL with a `Regex` constraint returns `InvalidRequest` immediately.
- `regex_constraint_is_rejected_when_streaming`: same for `complete_streaming`.
- `build_body_unchanged_for_supported_constraints`: JsonSchema, Lark, tools-only and unconstrained bodies equal the pre-change literals.

### T-12604 unit tests
- **Intent:** INT-0011
- `openai_tools_adapt_in_order`, `openai_tools_default_missing_parameters`, `openai_tools_reject_non_function`, `openai_tools_reject_duplicate_name`, `openai_tools_reject_reserved_control_name`, `openai_tools_reject_missing_name`, `adapted_tools_schema_has_one_branch_each`.

### T-12605 unit tests
- **Intent:** [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md)
- Mode selection: `constrained_transform_rewrites_request`, `passthrough_when_no_tools`, `passthrough_when_tool_choice_none`, `passthrough_when_response_format`.
- Body shape: `constrained_transform_inserts_system_when_absent`, `constrained_transform_forwards_unknown_fields`.
- History: `history_tool_call_projects_to_canonical_action`, `history_tool_message_projects_to_tool_result_text`, `history_final_answer_projects_to_task_complete`.
- Errors: `orphan_tool_call_id_is_rejected`, `non_object_arguments_are_rejected`.
- Determinism: `transform_is_deterministic`, `appended_turns_preserve_rendered_prefix`.

### T-12606 unit tests
- **Intent:** INT-0012
- `translate_tool_action`, `translate_final_answer`, `translate_length_is_not_parsed`, `translate_rejects_unparsable`, `translate_rejects_unoffered_tool`; SSE line reassembly `sse_lines_split_across_reads`.

### T-12607 unit tests
- **Intent:** INT-0012
- `receipt_has_required_fields` (including `message_hashes`), `receipt_marks_unavailable_metrics`, `default_listen_is_loopback`.

### T-12608 unit tests
- **Intent:** INT-0012
- `lab_root_inside_repo_is_refused`, `deadline_is_derived_from_measured_rates`, `admission_refuses_unknown_or_insufficient_memory` (pure functions in the example's support module).
- Moved memory-probe tests (`parse_meminfo` cases) run in `ferric-process` unchanged.

### T-12609 unit tests
- **Intent:** [INT-0013](../../../intents/INT-0013-constrained-decoding-on-midsize-quantized-target.md)
- `checker_lookup_accepts_expected_value`, `checker_lookup_rejects_other`, `checker_edit_detects_change`, `checker_create_requires_both_files`, `checker_no_tool_requires_zero_tool_calls` — each fed synthetic fixture states.

### T-12610 unit tests
- **Intent:** [INT-0010](../../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md)
- `landing_docs_state_iron_role`, `open_tasks_are_triaged`, `task_line_count_not_reduced` (against the pre-sprint count), `project_instructions_state_direction`.

## Integration Tests

### Valve against a scripted fake upstream (`crates/ferric-valve/tests/`, model-free, default lane)
- **Intents:** [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md), [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md)
- Fake upstream: a loopback axum/tokio server scripted per test (SSE chunks with delays, final `timings`, `finish_reason`, malformed output, stalls, connection-close observation).
- `stream_tool_call_end_to_end`, `stream_final_answer_end_to_end`, `stream_chunk_order`, `nonstream_equals_assembled_stream`.
- `heartbeat_only_on_upstream_progress`, `no_heartbeat_when_upstream_stalls`.
- `length_truncation_is_reported`, `malformed_upstream_yields_502`, `server_maps_transform_error_to_400`, `upstream_http_error_yields_502_with_status`, `upstream_unreachable_yields_502`.
- `client_disconnect_closes_upstream`, `cancelled_request_writes_cancelled_receipt`.
- `one_receipt_per_request`, `receipt_contains_no_message_text`.
- `probe_accepts_enforcing_upstream`, `probe_refuses_non_enforcing_upstream`.
- `record_only_is_byte_faithful_streaming`, `record_only_is_byte_faithful_nonstream`.
- `hermes_captured_request_round_trips`: a Hermes-shaped request fixture (system, user, assistant tool_calls, tool result, file-toolset `tools`) transforms, round-trips through the fake upstream and translates back to a valid OpenAI response.
- All tests bind ephemeral loopback ports, own their tasks, and finish within bounded timeouts; no child processes.

### Workspace regression
- **Intents:** INT-0011
- `cargo test --workspace --locked -- --test-threads=1` totals equal to the recorded baseline after T-12601 (`suite_totals_unchanged`), and green on Windows and Linux (WSL + CI) after every stage.

## End-to-End Tests
- **Status:** possible
- Runner: `cargo run -p ferric-valve --example hermes_pilot -- --lab <outside-repo> --hermes <Amalgam checkout> --python <Amalgam venv python> --server <llama-server b10964 CUDA> --model <gguf> --plan <plan>`; every child owned by `ProcessTree` and proven reaped.
- `bringup_7b_valve` (plan `smoke`, 7B): pass = at least one Hermes tool call executed and one final answer through the constrained valve; receipts present; each constrained request's `message_hashes` extends the previous one's within the session.
- `readiness_27b_valve` (plan `smoke`, 27B): same pass criteria on the reference model.
- `readiness_27b_cancel` (plan `cancel`, 27B): pass = after the interrupt file is written mid-generation, `/slots` reports idle within the derived bound and the valve receipt is `cancelled`.
- `pilot_27b` (plan `pilot`, 27B): pass = all 24 sessions accounted for (completed, failed, or excluded with reason), per-arm report and manifest published under `sprint-tests/pilot/`; the pilot has no performance pass threshold — it is measurement, and a negative result is a valid outcome.
- Results recorded in `docs/sprints/s126/sprint-tests/e2e-tests.md`.
