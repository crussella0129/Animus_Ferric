# Sprint 7 E2E / Acceptance Tests

**Status: possible, but the real-model paths require human-launched infrastructure
— this is the sprint's visual-heartbeat checkpoint (stop criterion #1).**

The AI-verifiable system test is already green; the real-model acceptance runs
are blocked on a running server / loaded GGUF that only the user can start.

## AI-verifiable system test (green, no human needed)
- **`cli::mock_query_end_to_end`** — `ferric query --mock` exercises the full
  loop → trace → guard → registry path end to end with zero model: it writes
  `ferric-mock.txt` through the real workspace boundary and emits exactly one
  parseable `q-*.jsonl` trace spanning `session_start..session_end(task_complete)`.
  This proves the harness plumbing is sound; it does not exercise a real model.

## Real-model acceptance (NEEDS HUMAN HEARTBEAT — not run)
These honor ADR-009 (no runtime/provider/grammar change merges without a traced
real-model run). They require the user to launch a server / provide a GGUF.

### E2E-1 — capability probe (the load-bearing one)
- **Setup (human):** `ollama serve` + `ollama pull <model>` (or `llama-server -m <gguf>`).
- **Run:** `ferric query --backend openai --protocol grammar --api-base http://localhost:11434/v1 --model <model> "<task>" --workspace <dir> --prompts-dir prompts`
- **Pass:** the returned assistant text validates against the unified action
  schema (a `{tool,args}` object with a valid tool const). **If the server
  silently ignores `response_format`, this FAILS — the HTTP constrained path is
  then reported as unverified rather than trusted.** This is the single check
  that confirms "the harness owns decoding" actually holds end-to-end.

### E2E-2 — L0 smoke, constrained over HTTP
- **Run:** the `l0_smoke` flow (`crates/ferric-cli/tests/l0_smoke.rs`) against the
  served model with `--protocol grammar`.
- **Pass:** a valid JSONL trace + the correct workspace file edit.

### E2E-3 — toolbench evidence (closes the s6 0.0% failure)
- **Run:** `ferric toolbench --backend openai --protocol grammar --model <1B> --api-base <url>`
  vs `--protocol native`, on a 1B GGUF served over HTTP.
- **Pass / the thesis made visible:** constrained fire rate ≈ 100% (the schema
  forces a valid tool const) vs the lower unconstrained native fire rate. This is
  the artifact that turns the s6 toolbench green for the *right* reason.
- `run_benchmarks.ps1` / `test_both_models.ps1` are the drivers (updated in T-006).

### mistral.rs in-process (TextXml)
- **Run:** `l0_smoke` native/TextXml variants with `--backend mistral` on
  `Llama-3.2-1B-Instruct-Q4_K_M.gguf` (the user has it in `D:\Models`).
- **Pass:** valid trace + correct edit; the 300 s engine kill-switch holds.
- The mistral.rs *constrained* E2E remains **blocked on the upstream
  llguidance/mistral.rs hang (ADR-020)** — explicitly deferred, not silently
  skipped. The constrained thesis is verified on the HTTP valve instead.

## Unlocks
A maintained local OpenAI-compatible server in the heartbeat loop unlocks
E2E-1..E2E-3. The in-process constrained path unlocks only when the upstream
hang is root-caused (backlog item, ADR-020).
