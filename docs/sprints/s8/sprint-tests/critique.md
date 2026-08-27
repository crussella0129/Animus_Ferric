# Test Critique — Sprint 8

> Self-critique against `prompts/test-critic.md`.

## Concerns

### C-001: T-804 lifecycle (spawn/kill/health-poll) has no unit test
- **Failure mode:** plan-test-mismatch
- **Response:** **defer-with-rationale.** Spawning/killing a real child and polling a live port can't be deterministically unit-tested cross-platform. The pure surfaces (argv/env, health URL, runfile serde, loopback pin) are fully tested; the real lifecycle is `e2e_server_up_toolbench` (heartbeat). Stated in the test-plan Notes.

### C-002: No model-free full `run_toolbench` → report integration test
- **Where:** `integration-tests.md`.
- **Failure mode:** weak-integration
- **Response:** **defer-with-rationale (recorded debt).** `run_toolbench` builds its provider via `create_provider` (real backends only) — there's no `--mock` path, so a model-free end-to-end run would need a refactor beyond this sprint. The `classify` + `render_report` + `summary_rows` units cover every branch of the logic the pipeline feeds; the full run is the E2E heartbeat. Debt: add a `--mock` toolbench path (or inject a provider) in a future sprint to close this.

### C-003: T-806 docs are grep-checked, not behaviour-tested
- **Failure mode:** weak-assertion
- **Response:** **reject.** Doc content is correctly verified by presence/grep (README section, `docs/testbench.md`, PS1 `server up`); there is no behaviour to assert.

## Confidence
`proceed-with-caveats` — every code EARS clause has a value-asserting unit test (137/0 green); the two deferrals are the inherently-non-unit-testable process lifecycle (heartbeat) and the recorded model-free-toolbench debt.
