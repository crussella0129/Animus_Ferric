# Plan Critique — Sprint 20

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: The 7B may fail early levels — "failed" sprint?
- **Failure mode:** outcome-anxiety
- **Response:** **the level reached IS the result.** `measured_level` = highest completed level; any value is a valid measurement and the first real data point for the full loop. The sprint's deliverable is *the wiring + a real run*, not a particular score. A low score would itself be a valuable finding (the multi-turn loop needs work) and would be recorded honestly.

### C-002: Refactor blast radius on a shared `Invocation`
- **Failure mode:** wide-change
- **Response:** **additive + 2 sites.** `openai: Option<_>` defaults the mistral/mock paths to identical behaviour; only `bench_cmd.rs:107` and `Invocation::mock()` add `openai: None`. Extracting `query_args` is a pure refactor covered by the new unit tests + the existing `bench_mock`/`l0_smoke`.

### C-003: The spawned child needs the `backend-openai` feature
- **Failure mode:** runtime-only-failure
- **Response:** **same as the toolbench.** The live run builds the binary `--features backend-openai` (documented); the `query_args` unit test + the `--mock` ladder need no feature, so CI stays green on the default build. A missing feature surfaces as the child's own clear error, not a silent hang.

### C-004: Is this on the rings north star?
- **Failure mode:** scope-drift
- **Response:** **directly on it.** `measured_level` is the capability signal that promotes a tier and *widens the rings* (ADR-019 → ADR-028/029). The full-loop bench is how a model *earns* that promotion ("demonstrated to reliably call stuff"). This unblocks the only producer of real measured_level on the constrained backend — completing the promotion machinery the rings depend on.

## Confidence
`clean` — an additive backend field + a pure, unit-tested arg builder; the mistral/mock paths and their tests are untouched; the live L0–L6 run is an honest measurement with every outcome valid.
