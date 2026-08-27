# Plan Critique — Sprint 16

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Only 2 rings exist, so the live sweep is thin
- **Failure mode:** weak-demo
- **Response:** **accept + mitigated.** The *mechanism* is forward-looking — it sweeps however many rings exist, so it exercises rings 2–3 the moment they land. The pure `recommend_max_ring` is unit-tested with 3+ rings, proving correctness ahead of the tools. And even the 2-ring live run is a real, useful artifact (it confirms a model is solid through ring 1).

### C-002: `--calibrate-rings` vs a plain `--max-ring`
- **Failure mode:** flag-conflict
- **Response:** **defined.** `--calibrate-rings` sweeps (it discovers the ceiling), so it supersedes a single `--max-ring` value when both are passed — documented. No silent ambiguity.

### C-003: Composes existing code rather than adding new capability
- **Failure mode:** thin-sprint
- **Response:** **that's the point.** Sprints 13–15 built the parts; this turns them into the *operator-facing payoff* of the whole rings thesis — "tell me the biggest ring my model can drive." A pure recommendation fn + a sweep + a report is a complete, valuable, AI-verifiable unit.

### C-004: `recommend_max_ring` semantics when ring 0 fails
- **Failure mode:** ambiguous-return
- **Response:** **`Option<u8>`, `None` = ring-0-not-solid.** The report turns `None` into an explicit "ring 0 not solid — the model can't reliably drive even the core; pick a stronger model or re-calibrate." No silent 0.

## Confidence
`clean` — a pure, well-tested recommendation fn + a sweep that reuses the proven `bench_model`/`verdict`; bounded; the live run is the headline artifact and the unit test covers the logic beyond today's ring count.
