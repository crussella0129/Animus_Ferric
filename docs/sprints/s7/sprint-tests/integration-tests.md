# Sprint 7 Integration Tests

Cross-unit behavior driven by `MockProvider` (no network) plus the build/feature
gates. All green within the per-task pre-commit gates.

## Component A+B+C — the loop under each protocol (`ferric-loop/tests/`, default via MockProvider)
- `constrained_loop::constrained_json_dispatches_tool` — mock returns `{tool,args}` JSON; loop in `ConstrainedJson` parses via `parse_json_action`, dispatches through `ferric-guard`/`ferric-tools`, frames the result as a user message, and the trace contains `constraint_applied` → `tool_call` → `tool_result` → `session_end(task_complete)`. ✅
- `constrained_loop::constrained_json_carries_action_schema` — every request carries `Constraint::JsonSchema` (an `anyOf` incl. `task_complete`) and empty tools (ADR-010). ✅
- `grammar_loop::textxml_happy_path` — mock returns `<tool_call>` XML; loop in `TextXml` parses via `parse_action`; trace has **no** `constraint_applied` (honesty). ✅
- `grammar_loop::textxml_request_shape` — TextXml requests carry no tools **and** no constraint. ✅
- `grammar_loop::{textxml_terminator_intercepted, textxml_malformed_action_rejected, textxml_repetition_guard}` — terminator interception, no-action nudge→EmptyCompletion, repetition warn→stop. ✅
- `truncation_tests::{truncated_once_then_recovers, truncated_twice_stops, truncation_distinct_from_parse_failure}` — ConstrainedJson truncation guard nudges once then `TruncatedAction`, distinct from parse-failure `EmptyCompletion`. ✅
- **Protocol matrix** reaches `StopReason::TaskComplete` under all three: NativeTools (`loop_core.rs`, unchanged), ConstrainedJson (`constrained_loop`), TextXml (`grammar_loop`). ✅

## Prompt composition (`ferric-prompt`, default)
- `compose_all_pairs` — all 6 tiers × **all 3 protocols** compose (the new `protocol-constrained-json` atom loads and version-pins). ✅
- `protocol_teaching_is_exclusive` — TextXml teaches `<tool_call>`; ConstrainedJson teaches the `{"tool","args"}` object; native teaches function-calling; each excludes the others. ✅

## CLI / bench (`ferric-cli`, default via MockProvider)
- `cli::mock_query_end_to_end` — full `ferric query --mock` system path: real guard+registry file write, exactly one parseable `q-*.jsonl` trace `session_start..session_end(task_complete)`. ✅
- `cli::query_without_backend_errors` — non-mock query in a backend-less build errors naming `mistralrs`. ✅
- `bench_mock` — bench result row records `protocol == "ConstrainedJson"` (the renamed default). ✅

## Component D — PyO3 removal (build/feature gates)
- `cargo build --features backend-openai,backend-mistralrs` compiles with no `pyo3`/`python` reference. ✅
- `cargo tree -p ferric-provider --all-features` → **0** pyo3 entries. ✅ (verified)
- `BackendArg` value-enum yields exactly `{mistral, openai}`. ✅

## Component F — doc-content checks (T-008, Test-phase grep)
- `decisions.md` matches `^## ADR-021` and `^## ADR-022` (count = 2). ✅ (verified)
- `README.md` Status no longer contains "Sprint 0" and names both backends. ✅ (verified)

## Notes
- `ferric-provider/tests/grammar_probe.rs` (the ADR-020 hang diagnostic) compiles again now that `Constraint` is reinstated; it is `#[ignore]` and only runs as a manual, bounded, real-GGUF subprocess (not part of CI).
