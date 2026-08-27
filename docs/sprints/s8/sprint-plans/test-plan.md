Finalized - DO NOT EDIT

# Sprint 8 Test Plan — The Self-Diagnostic Testbench

Tests derived from the build-plan EARS clauses. Default-graph + `cfg(test)` tests
run in CI; spawn/network parts are heartbeat-gated.

## Unit Tests

### T-801 — `classify` taxonomy (`toolbench_cmd.rs`, `cfg(test)`)
- `classify_success`: native/JSON completion calling target with valid args → `Outcome::Success`.
- `classify_wrong_tool`: completion calling a different tool → `Outcome::WrongTool(name)`.
- `classify_malformed_args`: target tool but a required key missing → `Outcome::MalformedArgs`.
- `classify_no_action`: prose with no action → `Outcome::NoAction`.
- `classify_parse_error`: action-shaped but unparseable (e.g. truncated JSON/XML) → `Outcome::ParseError`.

### T-802 — report rendering (`toolbench_cmd.rs`, `cfg(test)`)
- `render_report_contains_taxonomy_and_verdict`: a `BenchSummary` with mixed outcomes → Markdown contains each tool's rate, the failure histogram labels, and the overall verdict band.
- `summary_rows_jsonl_shape`: one object row per tool with `{tool, fires, success, rate, histogram}`.
- `verdict_bands`: `verdict(0.90)=="solid"`, `verdict(0.89)=="marginal"`, `verdict(0.70)=="marginal"`, `verdict(0.69)=="unreliable"`.

### T-803 — engine command/URL (`server.rs`, default)
- `llama_server_argv`: argv == `["-m", model, "-c", ctx, "--host", "127.0.0.1", "--port", port]` (+ `--mmproj p` when set); program == `llama-server`.
- `ollama_argv_and_env`: program `ollama` args `["serve"]`, env contains `OLLAMA_HOST=127.0.0.1:<port>`.
- `health_url_per_engine`: llama-server → `<base>/health`; ollama → `<base>/v1/models`.
- `host_is_loopback`: every engine's command pins `127.0.0.1` (ADR-005 — no `0.0.0.0`).

### T-804 / T-805 — runfile + discovery (`server.rs`/`backend.rs`, default)
- `runfile_serde_roundtrip`: `ServerRunfile` ser→de == original.
- `base_url_precedence`: explicit `--api-base` > runfile `base_url` > built-in default.
- `no_runfile_is_none`: `read_runfile` on a dir without `.ferric/server.json` → `None`.

## Integration Tests
- `toolbench_report_end_to_end` (MockProvider, default CI): script a provider to emit a known mix (e.g. 6× success, 2× wrong-tool, 2× no-action for one tool) → run the bench → assert the rendered report's per-tool counts, histogram, and overall verdict match; with `--report <tmp>` assert both `<tmp>.md` and `<tmp>.jsonl` exist with the expected rows.

## End-to-End Tests
- **Status:** possible — but the real runs need a model/server = **human heartbeat.**
- `e2e_server_up_toolbench`: `ferric server up --engine llama-server --model <gguf> [--mmproj <p>]` → `/health` green → `ferric server status` prints base_url → `ferric toolbench --backend openai --report report.md` produces a real diagnostic report (constrained vs native fire rate) → `ferric server down` stops it. This is the testbench made real.
- `e2e_mistralrs_0815_viability`: run `grammar_probe` (`trivial` then `unified`) against mistralrs **0.8.15** as a bounded subprocess on `Llama-3.2-1B`; record whether the ADR-020 constraint hang persists. **Decision gate (ADR-023):** if it returns within the bound, mistral.rs gains a real constrained path (keep/upgrade); if it still hangs, mistral.rs stays the TextXml-only fallback / deprioritized.
- Both are the user's visual heartbeat; `ferric server up` makes the first one-command.

## Notes
- Process spawn/kill (`server up`/`down`) and the health poll can't be deterministically unit-tested cross-platform; the **pure** parts (argv/env construction, health URL, runfile serde, base_url precedence) are fully covered, and the real spawn is the E2E heartbeat. This split is deliberate, not a coverage gap.
