# Sprint 2 Integration Tests — Results

Run 2026-06-14, local Windows: **ALL PASS**.

### ferric-loop UnifiedGrammar suite (`crates/ferric-loop/tests/grammar_loop.rs`, 5)
- `grammar_happy_path` — grammar-JSON text completions → action → dispatch; golden order incl. PolicySelected + ConstraintApplied; result framed as user-role `[tool_result for write_file]`; requests carry constraint + empty tools. **pass**
- `grammar_request_shape` — every request: tools empty AND constraint Some (ADR-010 unrepresentable-state proof). **pass**
- `grammar_terminator_intercepted` — task_complete never dispatched through the registry. **pass**
- `grammar_non_action_json_rejected` (C-012) — valid-but-non-action JSON → nudge → EmptyCompletion, never FinalText. **pass**
- `grammar_repetition_guard` — identical grammar actions → warned, stopped. **pass**

### ferric-loop truncation suite (`truncation_tests.rs`, 3)
- `truncated_once_then_recovers` — nudge "cut off"; partial JSON NOT in history. **pass**
- `truncated_twice_stops` — reason truncated_action. **pass**
- `truncation_distinct_from_parse_failure` — non-truncated garbage → empty_completion (modes stay distinguishable). **pass**

### ferric-loop native regression (`loop_core.rs` 7, `terminator_tests.rs` 5, `repetition_tests.rs` 3, `backoff_tests.rs` 3)
- All s1 suites pass with PolicySelected inserted into golden order + Completion-literal updates; `unknown_tool_feeds_back` (s1 critique C-003) intact. **pass**

### ferric-cli (`cli.rs` 6, `bench_mock.rs` 3)
- `mock_query_end_to_end` — default (grammar) mock query → exit 0, trace span, task_complete, file written through real guard. **pass**
- `bench_mock_l0_passes_and_writes_results`, `_records_each_requested_level`, `_keep_workspace_preserves_dir` — the full bench harness (spawn-self runner → verify → results.jsonl → calibrate) end-to-end, model-free. **pass**

### Carried s0/s1 suites
- `guarded_traced_execution`, `mock_loop_skeleton`, `tier_table_snapshot`, `builtin_file_tools` (now 10 incl. move/make_dir) — all pass.
