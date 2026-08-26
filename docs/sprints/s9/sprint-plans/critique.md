# Plan Critique — Sprint 9

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: T-901 fleet loop (provider-per-model sweep) isn't unit-tested
- **Failure mode:** plan-test-mismatch
- **Response:** **defer-with-rationale.** Looping `create_provider` per model needs real backends. The pure pieces — `bench_model` accumulation and `render_leaderboard` ordering — are unit-tested; the real sweep is the E2E fleet run (runnable now against ollama), which is the sprint's whole point. Not a gap.

### C-002: ADR-019 boundary (leaderboard vs `measured_level`)
- **Failure mode:** ignored-ADR (boundary)
- **Response:** **reject (kept distinct).** The leaderboard is a human-facing fire-rate readout; `measured_level` stays `ferric bench`'s product (ADR-019). The plan and test-plan both state this; the leaderboard JSONL never touches `model_profiles.json`.

### C-003: mistral.rs viability is a test-phase experiment, not a build task
- **Failure mode:** granularity / scope
- **Response:** **accept (correct placement).** It's a measurement that yields a decision (keep/deprioritize mistral.rs) + an ADR update at Loop close — test/loop work, not a code task. The native-`content` fallback (T-902) is the only rider that's actual code.

## Confidence
`proceed-with-caveats` — small, low-risk sprint (two code tasks + docs); the headline value is the E2E fleet run, fully runnable now.
