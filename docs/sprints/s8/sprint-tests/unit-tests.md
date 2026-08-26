# Sprint 8 Unit Tests

Derived from the build-plan EARS clauses; all ran green in each task's pre-commit
gate and in the final `cargo test --workspace` (137 passed / 0 failed).

## T-801 — failure taxonomy (`toolbench_cmd.rs`, `cfg(test)` → default CI)
- `classify_success_native`, `classify_success_constrained` — target tool + valid args → `Success`. ✅
- `classify_wrong_tool` — different tool → `WrongTool(name)`. ✅
- `classify_malformed_args` — target tool, missing required arg → `MalformedArgs`. ✅
- `classify_no_action` — native, no tool_calls → `NoAction`. ✅
- `classify_parse_error` — non-empty unparseable action text → `ParseError`. ✅
- `outcome_is_success` — `is_success()` true only for `Success`. ✅

## T-802 — report + verdict (`toolbench_cmd.rs`, `cfg(test)`)
- `outcome_labels` — each `Outcome` → its stable histogram label. ✅
- `verdict_bands` — `verdict(0.90)=solid`, `(0.89)=marginal`, `(0.70)=marginal`, `(0.69)=unreliable`. ✅
- `render_report_has_taxonomy_and_verdict` — Markdown has per-tool rate, the failure histogram (`no_action×1`, `malformed_args×5`), and all three verdict bands. ✅
- `summary_rows_shape` — one JSONL row per tool + an `__overall__` row with correct totals/verdict. ✅

## T-803/T-804 — server engine + runfile (`server.rs`, default CI)
- `llama_server_argv` — `llama-server -m … -c … --host 127.0.0.1 --port …`. ✅
- `llama_server_mmproj` — `--mmproj` included when set. ✅
- `ollama_argv_and_env` — `ollama serve` + `OLLAMA_HOST=127.0.0.1:<port>`. ✅
- `host_is_loopback` — every engine pins `127.0.0.1`, never `0.0.0.0` (ADR-005). ✅
- `health_url_per_engine` — llama-server `/health`, ollama `/v1/models`. ✅
- `runfile_serde_roundtrip`, `read_runfile_absent_is_none`. ✅

## T-805 — server auto-discovery (`backend.rs`, `cfg(test)`)
- `api_base_precedence` — explicit `--api-base` > runfile `base_url` > built-in default. ✅

## Gaps
None for T-801..T-805. T-806 is doc-content (grep-checked in integration). The
process spawn/kill/health-poll in T-804 is inherently not unit-testable
cross-platform — covered by the E2E heartbeat (see `e2e-tests.md`).
