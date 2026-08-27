Finalized - DO NOT EDIT

# Sprint 36 Test Plan

## Unit Tests

### T-3601 unit tests
- `drive_real`/CLI existing tests: unchanged, all pass (regression).
- `runs_loop_with_prebuilt_provider`: the extracted loop-execution function, called directly with
  a hand-built `MockProvider`, produces the same `LoopOutcome` shape `drive_mock` already produces
  for an equivalent script — proves it doesn't depend on `create_provider` internally.
- Stubs: `MockProvider` (already in `ferric-provider`).

### T-3602 unit tests
- `run_config_matches_inline_computation`: build a `RunConfig` via the extracted function for a
  fixed `(params_b, quant, ctx, family, backend, protocol_override)` tuple; assert its `protocol`
  equals calling `select_protocol` directly, and its `policy.tier`/`policy.max_output_tokens`
  equal calling `policy_for` directly, for the same inputs.
- `run_config_reused_across_calls`: build once, assert two separate reads of the returned config
  (e.g. `protocol`, `policy.max_ring`) are identical — a proxy for "safe to reuse, not consumed".

### T-3603 unit tests
- `parses_valid_request_line`: a JSON-RPC request string → typed `Request` with matching
  `method`/`id`/`params`.
- `parses_notification_without_id`: a line with no `id` field → recognized as a notification
  (dispatch produces no response value).
- `malformed_line_yields_parse_error`: invalid JSON → a `-32700` error response is constructed.
- Stubs: none (pure parsing functions).

### T-3604 unit tests
- `initialize_returns_fixed_version_and_tools_capability`.
- `tools_list_has_exactly_one_tool_named_ferric_query`.
- `ferric_query_schema_has_no_workspace_backend_or_model_field`: serialize the schema to
  `serde_json::Value`, assert `.get("properties").get("workspace"|"backend"|"model")` is `None`
  — the structural containment-guarantee regression test.

### T-3605 unit tests
- `tools_call_ferric_query_success`: scripted `MockProvider` (write_file → task_complete, same
  shape as `query.rs`'s existing `mock_provider` helper) → `isError:false`, `content[0].text`
  equals the mock's summary.
- `tools_call_unknown_tool_is_json_rpc_error`.
- `tools_call_provider_error_is_is_error_true_not_panic`: a provider stubbed to return
  `ProviderError`, assert the process/function does not panic and the result has `isError:true`.
- `tools_call_files_route_through_attach_fold_skip`: one case per `Attachment` branch
  (`AppendText`/`Media`/`Skip`), exercised against the shared file-routing function extracted in
  T-3605 (new fixtures authored here — `query.rs` has no existing `#[test]`/`mod tests` block
  today, confirmed by inspection, so there is nothing pre-existing to reuse).
- Stubs: `MockProvider`, temp files via `tempfile` (already a workspace dep).

## Integration Tests
### MCP dispatch integration
- `full_handshake_and_call_sequence`: drive `initialize` → `notifications/initialized` →
  `tools/list` → `tools/call` through the dispatch function in-process (a `Vec<Request>` in, not
  real stdio), with a scripted `MockProvider`; assert the final response's `content[0].text`
  matches the mock script's expected summary end to end.
- `error_then_success_same_session`: drive TWO `tools/call` requests through the same dispatch
  loop/session — first against a provider stubbed to error (assert `isError:true`, no panic), then
  a second, ordinary successful call immediately after (assert `isError:false` with the expected
  text) — proves the dispatch loop keeps serving correctly after an error response, not just that
  a single error doesn't panic in isolation.

## End-to-End Tests
- **Status:** possible (via `--mock`, no real GGUF model required).
- `e2e_mcp_stdio_subprocess`: spawn `ferric mcp --mock` as a real child process (`std::process::
  Command`), write JSON-RPC request lines to its stdin (initialize, tools/list, tools/call),
  read newline-delimited responses from its stdout, assert the `tools/call` response's text
  matches the mock script — proves real process framing (line delimiting, stdout purity, no log
  lines interleaved), not just the in-process dispatch function tested above.
- Real-model E2E (a live llama-server backing `ferric mcp` over the OpenAI backend) is a **manual
  verification step**, not automated: matches the project's established no-live-backend-CI
  position (ADR-045) — documented in the test report as "verified manually" if run, or noted as
  not run this sprint if not.

## Docs (T-3607)
- No automated test — ADR-046 and doc updates are reviewed manually against T-3607's EARS clause
  (states the exposed-tool-surface decision; explicitly flags chat-mode as still deferred), the
  same treatment prior ADR-only tasks (e.g. sprint 35's ADR-045) received. Noted explicitly here so
  the absence reads as a decision, not a gap.

## Build / Lint (all tasks)
- `cargo test --workspace` green.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all --check` clean.
