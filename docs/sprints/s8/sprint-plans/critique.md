# Plan Critique — Sprint 8

> Self-critique against `prompts/plan-critic.md`'s seven failure modes.

## Concerns

### C-001: T-804 lifecycle clauses (up/status/down/doctor) have no unit tests
- **Where:** `build-plan.md` T-804; `test-plan.md` T-804/T-805 only covers runfile serde + precedence.
- **Failure mode:** plan-test-mismatch
- **Why it matters:** four EARS clauses (spawn, health-check, kill, doctor) have no asserting unit test.
- **Response:** **defer-with-rationale.** Spawning/killing a child process and polling a live health endpoint cannot be deterministically unit-tested cross-platform without a real engine. The *pure* surfaces that carry the logic — argv/env construction, health URL, runfile serde, base_url precedence (T-803/T-805) — are fully unit-tested, and the real lifecycle is the E2E heartbeat (`e2e_server_up_toolbench`). This split is stated in the test-plan Notes, not a silent gap.

### C-002: T-804 is the heaviest task (subcommand + 4 lifecycle ops + runfile)
- **Where:** `build-plan.md` T-804.
- **Failure mode:** granularity
- **Why it matters:** four sub-behaviors in one task.
- **Response:** **reject (one coherent concern).** up/status/down/doctor all share the same `ServerRunfile` + `Engine` plumbing and only make sense together (a half-built lifecycle doesn't compile to anything usable). It is one coherent diff; the four EARS clauses are distinct behavioral surfaces of the one concern, which the schema permits.

### C-003: ADR-019 boundary (toolbench vs bench) — could the diagnostic toolbench drift into bench's job?
- **Where:** `build-plan.md` T-802 (writes a report + JSONL, like `bench`'s results.jsonl).
- **Failure mode:** ignored-ADR (boundary)
- **Why it matters:** both now produce JSONL reports; risk of conflating the human-facing fire-rate report with `bench`'s `measured_level` calibration.
- **Response:** **reject (kept distinct, reviewed).** The research's Decisions Reviewed calls this out: the toolbench is single-turn per-tool fire rate (a "is this model good enough" readout), `bench` is the L0–L6 trace-verified ladder that alone writes `measured_level` (ADR-019). The toolbench JSONL is its own `toolbench-*.jsonl` and never touches `model_profiles.json`/`results.jsonl`. The plan does not blur them.

## Confidence
`proceed-with-caveats` — every EARS clause maps to a test except T-804's inherently-untestable process lifecycle (deferred to the heartbeat with the pure parts covered). No blockers.
