# Sprint 8 Integration Tests

## CLI wiring (verified by build + smoke)
- `ferric --help` lists the new `server` subcommand alongside `query`/`bench`/`toolbench`/`trace`. ✅ (verified)
- `ferric server up --help` exposes `--engine/--model/--mmproj/--ctx/--port`. ✅ (verified)
- Smoke (no real engine): `ferric server status` → "no server registered" (exit 1); `ferric server down` → graceful no-op (exit 0). ✅ (verified)

## Cross-unit (report pipeline)
- The diagnostic report pipeline is covered by the pure-function unit tests
  (`render_report` + `summary_rows` over a hand-built `BenchSummary` with a mixed
  outcome histogram) — these exercise the exact path `run_toolbench` feeds. A
  `MockProvider`-driven full `run_toolbench` integration test was **not** added:
  `run_toolbench` constructs its provider via `create_provider` (real backends
  only), with no `--mock` path, so driving it model-free would require a refactor
  not in this sprint's scope. The classify + render + rows units fully cover the
  logic; the end-to-end run is the E2E heartbeat. (Recorded as debt below.)

## Component C — docs (T-806, grep-checked)
- `README.md` contains the "First run — the testbench" section. ✅ (verified)
- `docs/testbench.md` exists with the taxonomy table + verdict bands. ✅ (verified)
- `run_benchmarks.ps1` and `test_both_models.ps1` invoke `ferric server up`/`down`. ✅ (verified)

## Whole-workspace
- `cargo test --workspace`: **137 passed / 0 failed**; `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check` clean. ✅
- `backend-openai` feature: clippy + tests clean. ✅
