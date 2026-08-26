Finalized - DO NOT EDIT

# Sprint 16 Build Plan — Ring calibration (`toolbench --calibrate-rings`)

Bench a model ring-by-ring and report the highest ring it reliably drives — the
demonstrated-reliability promotion the user described, as one command. Composes
existing pieces (`bench_model`, `verdict`, `RunPolicy.max_ring`). Rationale:
`sprints/s16/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** measure the largest ring a model reliably drives.
  - **A. Sweep + recommendation** — T-1601
  - **B. Docs** — T-1602

## Execution Sequence

### T-1601: `--calibrate-rings` sweep + `recommend_max_ring`
- **Touches:** `crates/ferric-cli/src/toolbench_cmd.rs`
- **Success (EARS):**
  - `toolbench --calibrate-rings`: per model, sweep `ring_cap=0,1,…` (set `policy.max_ring=Some(ring_cap)`, re-derive `tools_for_policy`+`action_schema`, `bench_model`, record `(ring, rate, verdict)`); **stop** when a ring adds no new tools.
  - pure `recommend_max_ring(&[bool]) -> Option<u8>` = highest unbroken-`solid`-prefix ring; `None` if ring 0 not solid.
  - print `ring | tools | rate | verdict` + **"Recommended --max-ring N"** (or ring-0-not-solid note); `--report` → per-ring JSONL.
- **Notes:** reuse `bench_model`/`verdict`/`overall_rate`; sweeps per `--models` entry; unit-test `recommend_max_ring`.

### T-1602: Docs
- **Touches:** `README.md`, `docs/testbench.md`, `run_benchmarks.ps1`
- **Success (EARS):** calibration workflow doc ("largest ring → feed `--max-ring`/`measured_level`"); a `run_benchmarks.ps1` calibrate step; Sprint 16 timeline entry.

## Post-build (test)
- `recommend_max_ring` unit test + the E2E `--calibrate-rings` run vs ollama (both models → recommended `--max-ring 1`).
