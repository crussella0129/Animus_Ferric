# Sprint 8 Test Report

Sprint goal: the self-diagnostic testbench — a diagnostic `ferric toolbench`
(failure taxonomy + report + verdict) and a `ferric server` launcher.

## Summary
- **Unit + integration (default graph): 137 passed / 0 failed / 137 total** (`cargo test --workspace`; up from sprint 7's 122 — the ~15 new tests cover the taxonomy, report rendering, verdict bands, engine argv/health/runfile, and api-base precedence).
- **`backend-openai` feature:** clippy + tests clean.
- **E2E: 1 AI-verifiable (server smoke) / 0 failed; real-model acceptance N/A (human-heartbeat-gated)** — `ferric server status`/`down`/`up --help` behave; the real `up → toolbench → down` run and the mistral.rs 0.8.15 viability probe need a model/server (see `e2e-tests.md`).
- **CI status: local CI-equivalent GREEN; pushed.**
  - `cargo fmt --check` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ · `cargo test --workspace` 137/0 ✅ · `clippy -p ferric-cli --features backend-openai` ✅.
  - aarch64 cross-check: CI-gated (no new default-graph dep — `server.rs` is std-only; the launcher adds only `serde` to ferric-cli, already aarch64-clean).
  - Branch `sprint-7-realign` pushed to GitHub (sprint 7 + sprint 8, 18 commits). GitHub Actions runs on PR; not yet opened.

## Failures
None.

## Technical Debt Identified
- **No model-free `run_toolbench` integration test.** `run_toolbench` is hardwired to `create_provider` (real backends); it lacks a `--mock` path, so the full pipeline isn't exercised model-free (the pure report functions are). Add a mock/injectable provider path next.
- **mistral.rs constrained path still unresolved.** The 0.8.15 viability probe (the ADR-023 decision gate) is heartbeat-pending; until run, mistral.rs stays TextXml-only.
- **Cross-platform `down`** uses `taskkill`/`kill` subprocesses (no Child handle survives `up`); robust but not unit-verified — heartbeat covers it.

## Coverage Observations
- Every T-801..T-805 build-plan EARS clause maps to a value-asserting unit test; negative paths covered (wrong-tool, malformed-args, no-action, parse-error; absent runfile; base-url precedence).
- Security (ADR-005) is tested directly: `host_is_loopback` asserts every engine command pins `127.0.0.1` and never `0.0.0.0`; the engine is a closed enum (no arbitrary exec).
- The diagnostic verdict bands (solid/marginal/unreliable) are boundary-tested (89/90, 69/70) — the readout the user asked for is exact.
