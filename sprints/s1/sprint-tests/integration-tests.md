# Sprint 1 Integration Tests — Results

Run 2026-06-11, local Windows: **ALL PASS**.

### ferric-loop (`crates/ferric-loop/tests/`)
- `loop_core.rs` (6): `happy_path_golden_trace_order` (exact 11-event seq-ordered golden), `max_turns_budget` (exactly N turn_starts), `max_turns_still_emits_last_text` (best-effort output), `denied_tool_feeds_back` (deny traced via permission_check, error fed back, loop continues), `empty_completion_nudges_then_stops` (nudge visible in mock-recorded request), `adr010_request_shape` (every tool-turn request: tools non-empty AND constraint None). **all pass**
- `terminator_tests.rs` (5): `task_complete_terminates` (never dispatched, summary = final text), `task_complete_mixed_turn` (other calls execute first), `task_complete_always_offered` (beyond max_tools), `malformed_summary_still_terminates`, `terminator_result_not_fed_back` (loop ends after one request). **all pass**
- `repetition_tests.rs` (3): `repetition_warn_then_stop` (warned+stopped actions traced, nudge reaches the model), `repetition_resets_on_change`, `order_change_is_not_a_repeat`. **all pass**
- `backoff_tests.rs` (3): `backoff_schedule` (recorded 250/500/1000 ms), `backoff_exhaustion` (provider_error after 3 retries), `non_retryable_aborts_immediately` (zero sleeps). Uses a FlakyProvider (scripted Results) since MockProvider only scripts successes. **all pass**

### ferric-cli (`crates/ferric-cli/tests/cli.rs`)
- `mock_query_end_to_end`: exit 0, final text on stdout, exactly one `q-*.jsonl` spanning session_start→session_end(task_complete), and the mock's write_file landed through the real guard+registry. **pass**
- `query_without_backend_errors`: default build, non-mock → non-zero exit naming backend-mistralrs. **pass**
- `version_flag`, `trace_cat_renders_unknown`, `no_args_fails_with_usage`, `unknown_args_fail_with_usage`. **all pass**

### Carried s0 integration suites
- `guarded_traced_execution`, `mock_loop_skeleton`, `tier_table_snapshot`, `builtin_file_tools` — updated for CheckRecord-carrying outcomes; **all pass**.
