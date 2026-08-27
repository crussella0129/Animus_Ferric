# Sprint 36 Unit Tests

All derived from the locked `build-plan.md` EARS clauses (one test per WHEN/THEN/SHALL triple,
plus a regression per task). All green.

## T-3601 — separate provider construction from loop execution
- `query::tests::runs_loop_with_prebuilt_provider` — the extracted `run_with_provider`, called
  with a hand-built `MockProvider` (no `create_provider`), completes the loop (`TaskComplete`) and
  the mock's write lands on disk. Proves the "already-constructed provider, run one loop" clause.
- **Regression:** every existing `ferric query` test (mock + CLI) still passes — the refactor
  didn't change `ferric query`'s behavior (the "SHALL be unchanged" clause).

## T-3602 — launch-time-fixed run-config builder
- `query::tests::run_config_matches_inline_computation` — `build_run_config` produces the same
  `protocol` (`select_protocol`), `policy.tier`/`policy.max_output_tokens` (`policy_for`), AND
  `sampling.temperature`/`sampling.max_tokens` a direct call would, for identical inputs
  (temperature deliberately non-default, `0.7`, so the assertion isn't vacuous — test-critique
  C-003). Pins the extraction against drift.
- `query::tests::run_config_reused_across_calls` — the returned config is readable twice with
  identical values (proxy for "safe to reuse across many `tools/call`s, not consumed").

## T-3603 — JSON-RPC message types + stdio framing
- `mcp::tests::parses_valid_request_line` — a valid request line → typed `RpcRequest` with
  matching method/id; `is_notification()` false.
- `mcp::tests::parses_notification_without_id` — no `id` field → recognized as a notification.
- `mcp::tests::malformed_line_yields_parse_error` — invalid JSON → a `-32700` parse-error response
  with `id: null`.
- `mcp::tests::render_line_has_no_embedded_newline` — a serialized response contains no embedded
  `\n` (the stdout-framing invariant: one frame per line).
- `mcp::tests::no_bare_println_in_source` — a static source-scan guard (test-critique C-004): the
  module never contains a bare `println!` (stdout is reserved for frames; only `eprintln!` is
  used for diagnostics).

## T-3604 — initialize + tools/list
- `mcp::tests::initialize_returns_fixed_version_and_tools_capability` — fixed protocol version +
  a tools capability object + `serverInfo.name == "ferric"`.
- `mcp::tests::tools_list_has_exactly_one_tool_named_ferric_query` — exactly one tool, named
  `ferric_query`.
- `mcp::tests::ferric_query_schema_has_no_workspace_backend_or_model_field` — **the structural
  containment-guarantee regression** (ADR-046): the tool schema's `properties` has no
  `workspace`/`backend`/`model` key, and does have `prompt`.

## T-3605 — tools/call handler
- `mcp::tests::tools_call_ferric_query_success` — a valid `prompt` runs one loop → `isError:false`
  with the mock's final text.
- `mcp::tests::tools_call_unknown_tool_is_json_rpc_error` — an unknown *tool name* within a valid
  `tools/call` → a JSON-RPC `-32602` error (result absent), no loop attempted.
- `mcp::tests::dispatch_unknown_method_is_json_rpc_error` — an unrecognized *method* (a different
  code path from the above) → `-32601`, no dispatch attempted (test-critique C-005).
- `mcp::tests::tools_call_provider_error_is_is_error_true_not_panic` — a provider that errors →
  `isError:true`, no panic.
- `mcp::tests::tools_call_file_text_folds_into_prompt` — the `AppendText` branch: the injected
  file's content is verified (via the `mcp-*.jsonl` trace's `prompt_assembled` char count) to have
  actually folded into the prompt, not merely "the call didn't error" (test-critique C-002, which
  replaces the original weaker `tools_call_files_route_through_attach_fold_skip`).
- `mcp::tests::tools_call_file_media_skipped_with_reason` — the `Skip` branch (the
  security-relevant one, test-critique C-001): a media file with no declared modality is dropped
  non-fatally. (`Media`-attaches is untestable under `--mock`, which hardcodes
  `supports_media:false` — noted in the test's doc comment; parity with the existing CLI suite,
  which has the same gap.)

## Result
`cargo test -p ferric-cli` (lib): **42 passed / 0 failed**. `cargo test --workspace`: all crates
green; `clippy --workspace --all-targets -- -D warnings` and `fmt --check` clean (see
`test-report.md` for the full tally and `critique.md` for the critic pass that drove the C-001
through C-007 fixes above).
