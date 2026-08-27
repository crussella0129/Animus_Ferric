Finalized - DO NOT EDIT

# Sprint 9 Build Plan — Fleet Calibration

Cash in the validated testbench: bench the model fleet into a capability table.
Riders: native-`content` fallback (ADR-024) + mistral.rs 0.8.15 viability (test
phase). Multimodal → sprint 10. Rationale: `sprints/s9/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** fleet calibration — one command benches many models into a sorted leaderboard.
  - **A. Fleet sweep** — T-901
  - **B. Native robustness (ADR-024)** — T-902
  - **C. Docs + driver** — T-903

## Execution Sequence

### T-901: Fleet sweep — `--models` + leaderboard
- **Touches:** `crates/ferric-cli/src/toolbench_cmd.rs`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `ferric toolbench --models a,b,c` is given, **THEN** it **SHALL** bench each model (same backend; model overridden per run) and collect one `BenchSummary` per model.
  - **WHEN** the sweep finishes, **THEN** it **SHALL** print a leaderboard (Markdown `model | rate | verdict`, sorted best→worst) and, with `--report <path>`, write it plus a combined `<path>.jsonl`.
  - **WHEN** a single `--model` is given (no `--models`), **THEN** behaviour **SHALL** be unchanged.
- **Notes:** Extract `bench_model(provider, protocol, &all_tools, &schema, iterations) -> BenchSummary` (reuses `classify`/`build_request`/`ToolStat`). Fleet path loops the model list, `create_provider` per model (override `BackendOpts.model`/`model_file`). Pure `render_leaderboard(&[BenchSummary]) -> String` (sorted by `overall_rate`) unit-tested.

### T-902: Native-`content` fallback (ADR-024)
- **Touches:** `crates/ferric-provider/src/openai.rs`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** an OpenAI response has empty `tool_calls` AND `content` parses as a tool-call object (`{name,arguments}` or `{tool,args}`), **THEN** the backend **SHALL** synthesize a `ToolCall`.
  - **WHEN** `content` is ordinary prose, **THEN** no `ToolCall` **SHALL** be synthesized.
- **Notes:** Pure `toolcall_from_content(content: &str) -> Option<ToolCall>` (tolerates `arguments` as JSON string or object), applied only when `tool_calls` is empty. Unit-tested model-free. Fixes the ADR-024 native-on-ollama 0%.

### T-903: Docs + fleet driver
- **Touches:** `README.md`, `docs/testbench.md`, root `run_benchmarks.ps1`
- **Depends on:** T-901, T-902
- **Success criterion (EARS):**
  - **WHEN** `docs/testbench.md` is read, **THEN** it **SHALL** document the fleet sweep (`--models …`) + reading the leaderboard to pick the smallest viable model.
  - **WHEN** `run_benchmarks.ps1` is read, **THEN** it **SHALL** include a fleet sweep across the installed ollama models.
