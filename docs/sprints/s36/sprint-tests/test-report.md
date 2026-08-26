# Sprint 36 Test Report — `ferric mcp` (ADR-046)

## Summary
- Unit tests: 42 passed / 0 failed / 42 total (`ferric-cli` lib — 22 pre-existing + 20 covering
  T-3601–T-3605, incl. the 4 test-critic add/tighten fixes: sampling-value coverage, the
  Skip-branch + trace-verified AppendText-branch file routing, unknown-method `-32601`, and the
  `no_bare_println_in_source` source guard).
- Integration tests: 2 passed / 0 failed / 2 total (`full_handshake_and_call_sequence`,
  `error_then_success_same_session`).
- E2E tests: 1 passed / 0 failed / 1 total (`cli::mcp_stdio_e2e` — a real `ferric mcp --mock`
  subprocess, now also covering the malformed-line negative path and hardened against hangs).
- Full workspace (`cargo test --workspace`): **all crates green**, no regressions in any
  previously-shipped sprint's tests.
- Lint/format: `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all
  --check` clean.
- CI status: **not run this sprint** (`.github/workflows/ci.yml` exists; will run on push/PR as
  usual — no repo-specific CI change was needed for this sprint's work).

## Failures
None. No test failed at any point in this sprint's Test Phase; the seven test-critic concerns
(see `critique.md`) were coverage/assertion-strength gaps in an otherwise-passing suite, not
failing tests.

## Technical Debt Identified
- **`Media`-successfully-attaches path remains untested** at both the MCP and CLI layers: `--mock`
  hardcodes `caps.supports_media = false`, so the attach branch is only reachable with a real,
  capability-declaring backend. Not a regression from this sprint (parity with prior CLI coverage);
  would need a live-backend or a purpose-built capability-injecting test double to close, which is
  out of scope here.
- **Real-model E2E is manual, not automated** (`e2e-tests.md`) — consistent with the project's
  established no-live-backend-CI position (ADR-045); the `--mock` subprocess E2E covers the
  transport/lifecycle deterministically instead.
- **`ferric mcp`'s launch-time-fixed profile staleness** (ADR-046, documented, not a test gap): a
  `ferric bench --calibrate-rings` run against an already-running server is picked up only on
  restart. No test needed beyond the existing `run_config_reused_across_calls` proof that the
  config doesn't silently re-derive.

## Coverage Observations
- Every EARS clause in the locked `build-plan.md` now has a corresponding test, including all
  error/negative paths (parse error, unknown method, unknown tool, provider error, malformed-line
  mid-session) — closing the gaps the test-critic identified (`critique.md`, C-001–C-007).
- The structural containment guarantee (ADR-046's core security property — no
  `workspace`/`backend`/`model` field on the exposed tool schema) has a dedicated regression test
  (`ferric_query_schema_has_no_workspace_backend_or_model_field`), so a future change that
  accidentally widens the schema would fail CI immediately.
- The E2E test now exercises the two properties that matter most for a long-running server process
  under real usage: **surviving a malformed request** and **exiting cleanly on EOF** — both driven
  through the actual OS pipes, not simulated in-process.
