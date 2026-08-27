# Sprint 7 Unit Tests

Derived from the build-plan's EARS clauses (one `test_*` per WHEN/THEN/SHALL).
All ran green within each task's pre-commit gate (`cargo clippy --all-targets -D
warnings` + `cargo test`). Default-graph tests run in CI's primary job;
`backend-openai` tests run under that feature locally.

## T-001 — Constraint contract (`ferric-provider/src/types.rs`, default)
- `validate_rejects_constraint_and_tools` — constraint + tools → `Err(InvalidRequest)` (ADR-010). ✅
- `validate_accepts_lawful_combinations` — constraint-only ✅, tools-only ✅, neither ✅ → `Ok`.
- `constraint_jsonschema_serde_roundtrip` — `Constraint::JsonSchema` ser→de == original. ✅

## T-002 — HTTP `response_format` (`ferric-provider/src/openai.rs`, `backend-openai`)
- `build_body_constraint_emits_response_format` — `JsonSchema(s)` → `response_format.json_schema.schema==s`, `strict:true`, no `tools`. ✅
- `build_body_tools_no_response_format` — tools + no constraint → `tools`/`tool_choice`, no `response_format`. ✅
- `capabilities_advertise_constraint_and_native` — `supports_constraint && supports_native_tool_calls`. ✅

## T-003 — action schema + JSON parser (`ferric-loop/src/grammar.rs`, default)
- `action_schema_branch_count` — N tools → N+1 `anyOf` branches, each `additionalProperties:false`, `required:[tool,args]`. ✅
- `action_schema_includes_task_complete` — a `tool.const=="task_complete"` branch is present. ✅
- `parse_json_action_happy` — `{"tool":"read_file","args":{...}}` → `ToolCall{read_file, id g-2-0}`. ✅
- `parse_json_action_rejects_non_object` — `"oops"` → `Err`. ✅
- `parse_json_action_rejects_missing_tool` — `{"args":{}}` → `Err(MissingTool)`. ✅
- `parse_json_action_rejects_missing_args` — `{"tool":"read_file"}` → `Err(MissingArgs)`. ✅

## T-004 — protocol trichotomy (`ferric-loop/src/protocol.rs` + `ferric-core/src/scale.rs`, default)
- `constraint_capable_selects_constrained_json` — caps{constraint} → `ConstrainedJson`. ✅
- `native_only_selects_native_tools` — caps{native, !constraint} → `NativeTools`. ✅
- `neither_selects_text_xml` — caps{!native, !constraint} → `TextXml`. ✅
- `override_always_wins` — explicit override returned in all three directions. ✅
- `action_protocol_serde` — `constrained_json`/`text_xml` round-trip; legacy `unified_grammar` alias deserializes to `ConstrainedJson`. ✅

## T-007 — toolbench parse dispatch (`ferric-cli/src/toolbench_cmd.rs`, `cfg(test)` → default CI)
- `native_path_reads_tool_calls` — native completion → `tool_calls[0].name`. ✅
- `constrained_path_parses_json` — JSON text → `parse_json_action` name. ✅
- `textxml_path_scrapes_xml` — `<tool_call>` text → `parse_action` name. ✅
- `no_action_is_a_miss` — prose → `None` for all three protocols. ✅

## Gaps
None. Every T-001..T-004 + T-007 EARS clause maps to a named test. T-005/T-006
(PyO3 removal) are build-gate assertions (compile-clean + `cargo tree`
pyo3-count 0), recorded in `integration-tests.md`; T-008 is a doc-content check
recorded there too.
