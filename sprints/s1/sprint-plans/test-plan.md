Finalized - DO NOT EDIT

# Sprint 1 Test Plan — First Real Backend + Production Loop

## Unit Tests

### T-101 unit tests
- `new_events_roundtrip`: serialize/deserialize each of the six new Event variants → equal.
- `s0_trace_still_parses`: s0-format fixture (old event set) → all Known, no errors.
- `turn_end_carries_completion`: TurnEnd with text + token counts round-trips.
- Stubs: none.

### T-102 unit tests
- `check_records_on_allow`: completed execute → one CheckRecord per target, decision Allow.
- `check_records_on_deny`: denied execute → CheckRecord carries rule + matched.
- `ferric_dir_write_denied`: write under `.ferric/trace/x.jsonl` → Deny, rule denied_write_segment.
- `denylist_is_const` (extended): `.ferric` present in DENIED_WRITE_SEGMENTS.
- Stubs: flag tool, tempfile workspaces.

### T-103 unit tests
- `validate_matrix`: none / constraint-only / tools-only → Ok; both → InvalidRequest naming the conflict.
- `retryability_per_variant`: only RetryableBackend → true.
- Stubs: none.

### T-109 unit tests (feature-gated; model-free; compiled by CI backend-check, run locally)
- `constraint_maps_one_to_one`: each ferric Constraint → corresponding mistralrs::Constraint.
- `messages_map_including_tool_turns`: system/user/assistant + tool-call/tool-result turns → RequestBuilder shape.
- `sampling_maps_with_deterministic_switch`: temperature 0.0 → deterministic sampler; max_tokens → set_sampler_max_len.
- `both_set_rejected_before_engine`: constraint + tools → InvalidRequest (no engine contact).
- Stubs: none (free functions).

### T-110 unit tests
- `trace_cat_output_unchanged`: s0 fixture → byte-identical output vs recorded golden.
- `usage_on_no_args`: exit non-zero, clap usage on stderr.
- Stubs: tempfile fixtures, CARGO_BIN_EXE.

### T-111 unit tests
- (covered by CLI integration tests below — binary-level.)

### T-112 / T-113
- T-112 is the E2E itself. T-113 verified by inspection: decisions.md has ADR-010..014; backlog has every research-§5.5 item sprint-tagged.

## Integration Tests

### ferric-loop (the bulk — `crates/ferric-loop/tests/`, MockProvider + futures-executor)
- `happy_path_golden_trace_order`: tool call → final text; exact seq-ordered event-kind list asserted (T-104 EARS).
- `task_complete_terminates`: never dispatched; summary = final text; reason task_complete.
- `task_complete_mixed_turn`: other calls execute first, then terminate.
- `task_complete_always_offered`: 10 registered tools + NANO max_tools → descriptors include task_complete.
- `repetition_warn_then_stop`: identical-twice → warned event + nudge visible in mock-recorded request; thrice → stopped.
- `repetition_resets_on_change`: arg change → counter resets, no stop.
- `backoff_schedule`: scripted retryable failures → recorded sleeps 250/500/1000; then success completes.
- `backoff_exhaustion`: 4th failure → ProviderError stop.
- `non_retryable_aborts_immediately`: zero sleeps.
- `max_turns_budget`: N+1 tool turns under max_turns=N → MaxTurns; exactly N turn_starts; last assistant text still emitted.
- `denied_tool_feeds_back`: .git/config write → permission_check deny traced, is_error result fed back, loop continues.
- `adr010_request_shape`: every tool-turn request recorded by mock has tools non-empty AND constraint None.
- `empty_completion_nudges_then_stops`: completion with neither text nor calls → one nudge, then reason empty_completion.

### ferric-cli integration (`crates/ferric-cli/tests/cli.rs` extensions)
- `mock_query_end_to_end`: `ferric query --mock "x"` in temp dir → exit 0, final text on stdout, trace exists at `.ferric/trace/q-*.jsonl`, parses, spans session_start..session_end.
- `query_without_backend_errors`: non-mock query in default build → exit non-zero, message names backend-mistralrs.
- `trace_cat_regression`: s0 + s1 events render with no unknown fallbacks.

## End-to-End Tests
- **Status:** POSSIBLE (first sprint where it is).
- `l0_smoke` (`crates/ferric-cli/tests/l0_smoke.rs`, `#[ignore]` + feature-gated, env FERRIC_SMOKE_MODEL_DIR/FILE, Llama-3.2-1B-Instruct Q4_K_M, temperature 0):
  1. Process exits 0.
  2. `<ws>/hello.txt` content exactly `hello ferric` (trim_end-tolerant).
  3. Exactly one `.ferric/trace/q-*.jsonl`; every line parses; all v==1; seq strictly monotonic from 0.
  4. First event session_start (workspace == tempdir); last session_end with reason ∈ {task_complete, final_text}.
  5. ≥1 turn_start/turn_end pair; some turn_end has output_tokens > 0.
  6. ≥1 tool_call write_file with path hello.txt; matching tool_result is_error == false; matching allow permission_check.
  7. prompt_assembled lists write_file and task_complete among offered tools.
  8. Total turns ≤ NANO max_turns (15); wall time + token counts printed (recorded into test-report as the load/RSS actuals).
- **Still not possible:** aarch64 runtime verification (check-only gate); L1–L6 calibration (benchmark harness ports s2); HTTP escape-valve backend; streaming; GPU features; 7B quality run is best-effort manual, not a gate.
- CI is model-free: smoke is double-excluded (feature off + #[ignore]); ADR-009 satisfied by the mandatory local run before merge of any provider/loop/constraint change.
