Finalized - DO NOT EDIT

# Sprint 2 Test Plan — Prompts, Unified Action Grammar, Calibration

## Unit Tests

### T-201 unit tests
- Build gates: default `cargo check --workspace` (no mistralrs/tokio in graph — `cargo tree` assertion), aarch64 check, Cargo.lock pins oovra rev 378abea.
- `preserve_order_active`: serde_json object built by insertion serializes in insertion order.

### T-202 unit tests
- `tier_table_snapshot` extended: max_output_tokens 512/768/1024/1536/2048/2048 pinned per tier.
- `action_protocol_serde`: round-trips as "native_tools"/"unified_grammar".

### T-203 unit tests
- `new_events_roundtrip` extended: PolicySelected + PromptComposed parse as Known, v stays 1.
- `trace_cat` renders both with no `[unknown event]` fallback.

### T-204 unit tests
- `finish_reason_maps_truncated`: "length" → true; "stop"/"tool_calls"/"canceled" → false (free function).
- Mock scripting: truncated settable, defaults false.

### T-205 unit tests
- `move_path_renames_file` and `_dir`; `move_path_missing_source_is_error` (no panic); `move_path_outside_to_denied` (both endpoints checked, no rename); `make_dir_creates_parents` + idempotent; `.ferric` denial applies to both endpoints; sorted tool listing holds with five builtins.

### T-206 unit tests
- `schema_golden`: serialized schema contains anyOf, x-guidance whitespace_flexible:false, N+1 branches, additionalProperties:false at both depths; string "oneOf" ABSENT.
- `tool_precedes_args_in_serialization` (preserve_order regression pin).
- `parse_action_roundtrip`; `parse_action_rejects_garbage` (typed error, no panic); `parse_action_rejects_partial_json`.

### T-209 unit tests
- `compose_all_pairs`: every tier × protocol returns Ok with lineage == recipe_for.
- `grammar_prompt_teaches_grammar_only` / `native_prompt_teaches_native_only` (protocol-exclusive content).
- `missing_library_is_typed_err`.

### T-211 unit tests
- `all_seven_specs_parse`; `unknown_field_rejected` (deny_unknown_fields); L0 forbidden set includes write_file/move_path/make_dir; L1/L2 use Ferric tool names.

### T-213/T-214 unit tests
- `verdict_matrix`: handcrafted trace fixtures → completed truth table (timeout, exit, expectations, tools required/any_of/forbidden, terminator).
- `results_append_not_truncate`; `measured_level_is_highest_completed`; rows carry tier_from_params + tier_from_measured.

## Integration Tests

### ferric-loop UnifiedGrammar suite (`crates/ferric-loop/tests/grammar_loop.rs`) — the bulk
MockProvider scripted with grammar-shaped JSON TEXT completions (tool_calls empty):
- `grammar_happy_path`: write_file action then task_complete action → TaskComplete; golden event order incl. PolicySelected + ConstraintApplied; EXPLICIT assertion (C-006) that the second mock-recorded request contains a user-role message starting `[tool_result for write_file]` (result framing is load-bearing).
- `grammar_non_action_json_rejected` (C-012): a syntactically valid JSON completion WITHOUT {tool,args} shape → typed rejection via the empty-completion path, never treated as FinalText.
- `select_protocol_matrix` (C-001, unit): ConstrainedJson+constraint-capable → UnifiedGrammar; constraint-incapable → NativeTools; CLI override wins.
- `grammar_terminator_intercepted`: task_complete action never dispatched through Registry.
- `grammar_truncated_once_nudges_twice_stops`: reason truncated_action.
- `grammar_unparseable_falls_back`: non-truncated garbage → empty-completion path.
- `grammar_repetition_guard`: identical grammar actions trip warn→stop.
- `grammar_request_shape`: recording provider asserts constraint Some + tools EMPTY on every request (ADR-010 unrepresentable-state proof).
- `native_mode_regression`: s1 suites pass unmodified except Completion literals.

### ferric-cli (`crates/ferric-cli/tests/`)
- `mock_query_grammar_end_to_end`: `ferric query --mock --protocol grammar` → exit 0, trace spans session frame with PolicySelected, task_complete reason. (Mock script gains a grammar-shaped variant.)
- `bench_mock.rs`: fixture spec matching the built-in mock script written to a tempdir → `CARGO_BIN_EXE_ferric bench --mock --specs-dir <tmp> --results-dir <tmp2>` → row fields asserted, exit codes; timeout-0 fixture → timed_out row; --keep-workspace preserves dir.

## End-to-End Tests
- **Status: POSSIBLE** (second sprint with a real backend).
- `l0_smoke` (native — s1 assertions + PolicySelected present) and `l0_smoke_grammar` (--protocol grammar; same eight assertions, terminator ∈ {task_complete, final_text} for BOTH — the grammar's terminator effect is measured by the sweep, not pre-asserted by the gate; C-010).
- **Calibration sweep (manual, release, ADR-009):** `ferric bench --level 0..4 --protocol grammar` then `--protocol native` on Llama-3.2-1B; Qwen2.5-Coder-7B informational/non-blocking (C-010). results.jsonl + model_profiles.json committed under benchmarks/ as the sprint's empirical record, including per-protocol task_complete-rate comparison. Answers: does the grammar fix the terminator failure; does grammar mode regress native-format models.
- **Still impossible without a model:** actual llguidance mask enforcement (smoke-grammar is the schema-compile acceptance gate), real finish_reason emission, prompt-quality deltas, calibration numbers.
