Finalized - DO NOT EDIT

# Sprint 21 Build Plan — Fleet agentic capability map (`bench --models`)

Sprint 20 validated the full loop + found qwen-7b maxes L0–L6. Run the now-working
full-loop bench across the fleet to map each model's agentic `measured_level`
(does a 1B *complete* multi-turn tasks?) — and learn whether L7+ are needed yet.
Mirror the toolbench fleet sweep. Rationale: `sprints/s21/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** per-model agentic measured_level across the fleet.
  - **A. bench --models sweep** — T-2101
  - **B. run the fleet + docs** — T-2102

## Execution Sequence

### T-2101: `bench --models` fleet sweep
- **Touches:** `crates/ferric-cli/src/bench_cmd.rs`
- **Extract:** the per-level loop (`bench_cmd.rs:159–231`) → `run_levels(&selected, &inv, &args, model_label) -> (Vec<ResultRow>, bool)`; single path calls it unchanged.
- **Success (EARS):** `bench --models <a,b,c>` (openai) → per model id: openai `Invocation`, `run_levels`, `calibrate`+`write_profile`; then a `model | measured_level | tier` leaderboard sorted by level desc. Exit non-zero only on a runner error. No `--models` ⇒ single path byte-identical.
- **Tests:** `bench_mock`/`l0_smoke` stay green.

### T-2102: Run the fleet + docs
- **Touches:** `decisions.md`, `README.md`, `docs/testbench.md`, `run_benchmarks.ps1`
- **Success (EARS):** `bench --backend openai --models qwen2.5-coder:7b,llama3.1:8b,llama3.2:1b --protocol grammar` → per-model measured_level + leaderboard; ADR-030 amendment (fleet map); docs for `bench --models`; note re L7+.

## Post-build (test)
- `bench_mock`/`l0_smoke` regression + the live fleet run (capability map → does the 1B complete tasks?).
