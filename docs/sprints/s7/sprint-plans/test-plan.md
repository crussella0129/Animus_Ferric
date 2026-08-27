Finalized - DO NOT EDIT

# Sprint 7 Test Plan — Re-align Ferric to the Constrained-Decoding Thesis

Tests are derived from the build-plan's EARS clauses (one `test_*` per
WHEN/THEN/SHALL triple). Default-graph tests run in CI; backend tests are
feature-gated and run locally per ADR-009.

## Unit Tests

### T-001 unit tests (`ferric-provider/src/types.rs`, default graph)
- `test_validate_constraint_and_tools_rejected`: `constraint=Some, tools=[t]` → `Err(InvalidRequest)`.
- `test_validate_constraint_only_ok`: `constraint=Some, tools=[]` → `Ok`.
- `test_validate_tools_only_ok`: `constraint=None, tools=[t]` → `Ok`.
- `test_validate_neither_ok`: `constraint=None, tools=[]` → `Ok`.
- `test_constraint_jsonschema_serde_roundtrip`: `Constraint::JsonSchema(v)` ser→de == original.
- Stubs: none (pure types).

### T-002 unit tests (`ferric-provider/src/openai.rs`, feature `backend-openai`)
- `test_build_body_constraint_emits_response_format`: `Constraint::JsonSchema(s)` → body has `response_format.json_schema.schema == s`, `strict==true`, and no `tools` key.
- `test_build_body_tools_no_response_format`: tools + no constraint → body has `tools`/`tool_choice:auto`, no `response_format`.
- `test_openai_capabilities_flags`: `supports_native_tool_calls==true && supports_constraint==true`.
- Stubs: none (pure body builder; no network).

### T-003 unit tests (`ferric-loop/src/grammar.rs`/`action.rs`, default graph)
- `test_action_schema_branch_count`: N tools → `anyOf.len()==N+1`, each branch has a `tool.const` and `additionalProperties:false`.
- `test_action_schema_includes_task_complete`: a branch with `tool.const=="task_complete"` is present.
- `test_parse_json_action_happy`: `{"tool":"read_file","args":{"path":"x"}}` → `ToolCall{name:"read_file", id:"g-<turn>-0"}`.
- `test_parse_json_action_not_object`: `"\"oops\""` → `Err`.
- `test_parse_json_action_missing_tool`: `{"args":{}}` → `Err`.
- `test_parse_json_action_missing_args`: `{"tool":"read_file"}` → `Err`.

### T-004 unit tests (`ferric-loop/src/protocol.rs`, default graph)
- `test_select_constrained_when_supports_constraint`: caps{constraint:true} → `ConstrainedJson`.
- `test_select_native_when_only_native`: caps{constraint:false, native:true} → `NativeTools`.
- `test_select_textxml_when_neither`: caps{constraint:false, native:false} → `TextXml`.
- `test_select_override_wins`: explicit override returned verbatim for all three.
- Stubs: hand-built `Capabilities`, `RunPolicy` via `policy_for`.

## Integration Tests

### Component A+B+C integration (`ferric-loop/tests/`, default graph via MockProvider)
- `test_loop_constrained_json_dispatches_tool`: `MockProvider` scripted to return text `{"tool":"write_file","args":{...}}`; loop in `ConstrainedJson` parses via `parse_json_action`, dispatches through `ferric-guard`/`ferric-tools`, and the trace contains `ConstraintApplied` then `ToolResult` then `SessionEnd(task_complete)`. (Extends `mock_loop_skeleton.rs`.)
- `test_loop_textxml_no_constraint_event`: `MockProvider` returns `<tool_call>` XML; loop in `TextXml` parses via `parse_action`, and the trace **does not** contain `ConstraintApplied`.
- `test_loop_native_reads_tool_calls`: `MockProvider` returns native `tool_calls`; loop in `NativeTools` dispatches them.
- `test_protocol_matrix_reaches_task_complete`: one scripted task reaches `StopReason::TaskComplete` under each of the three protocols.

### Component D integration (build/feature gates)
- `test_no_pyo3_in_tree` (CI script step): `cargo tree -p ferric-provider --all-features` contains no `pyo3`.
- `test_backend_arg_variants`: `BackendArg` value-enum yields exactly `{mistral, openai}` (compile + `--help` snapshot).

### Component F doc-check (T-008 — Test-phase grep, C-001)
- `check_adrs_present` (Test-phase grep, not a unit test): `decisions.md` matches `^## ADR-021` and `^## ADR-022`.
- `check_readme_status_updated`: `README.md` Status section no longer contains "Sprint 0" / "No inference backend yet" and names both backends.
- Rationale: these are documentation-content assertions; verified by inspection/grep in the Test phase rather than `cargo test`.

### Component E integration (`ferric-cli`, MockProvider-driven)
- `test_toolbench_counts_native_pass`: completion with matching native `tool_call` → pass.
- `test_toolbench_counts_json_pass`: completion text `{"tool":<target>,"args":{}}` under `ConstrainedJson` → pass.
- `test_toolbench_counts_xml_pass`: completion text `<tool_call>...<name>target</name>...` under `TextXml` → pass.
- `test_toolbench_counts_miss_as_fail`: completion with no extractable matching call → fail.

## End-to-End Tests
- **Status:** possible — but the constrained path requires a running OpenAI-compatible server (Ollama / `llama-server`). **Human-launched: this is the visual-heartbeat checkpoint.**
- `e2e_capability_probe`: `ferric query --backend openai --protocol grammar --api-base <url> --model <m> "<task>"` issues one constrained request; assert the returned action text validates against `action_schema` (proves the server honors `response_format`; ADR-009 gate for the HTTP path). **If the server silently ignores `response_format`, this E2E fails and the HTTP constrained path is reported as unverified rather than trusted.**
- `e2e_l0_smoke_constrained`: the `l0_smoke` flow over HTTP+constraint writes a valid JSONL trace and the correct workspace file edit.
- `e2e_toolbench_evidence`: on a 1B GGUF served over HTTP, constrained fire rate ≈100% vs unconstrained native fire rate — the thesis made visible (this is the artifact that closes the s6 0.0% failure).
- `e2e_mistralrs_native_unaffected`: existing `l0_smoke` native/TextXml variants stay green with mistral.rs (no constraint; 300 s kill-switch honest).
- Unlocked fully by: a maintained local server in the heartbeat loop; the in-process mistral.rs *constrained* E2E remains blocked on the upstream llguidance fix (backlog), and that block is explicit, not silent.
