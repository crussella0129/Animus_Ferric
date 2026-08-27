# Plan Critique — Sprint 21

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Is this "just more running"?
- **Failure mode:** thin-sprint
- **Response:** the *question* is the value: can a 1B **complete** multi-turn agentic tasks, or only fire single tool calls? That's the core Ferric thesis at the loop level, unanswered until now (the bench only just gained the constrained backend, s20). The code (extract `run_levels` + a `--models` loop + leaderboard) is small precisely because s20 did the hard part; the payoff is the capability map + the L7+ decision.

### C-002: Extraction could change the single-model path
- **Failure mode:** refactor-regression
- **Response:** `run_levels` is a literal lift of lines 159–231; the single path calls it with the same `inv`. `bench_mock`/`l0_smoke` pin that the mock ladder is byte-identical. The fleet branch is purely additive (guarded by `--models`).

### C-003: `--models` openai-only
- **Failure mode:** scope-cut
- **Response:** deliberate + matches reality — a fleet is ollama model ids (the toolbench fleet is the same). A GGUF "fleet" via mistral is degenerate (constrained hangs, ADR-027) and out of scope. Documented; the single mistral/mock paths are unchanged.

### C-004: A low measured_level looks like failure
- **Failure mode:** outcome-anxiety
- **Response:** a low level **is** the measurement — e.g. "the 1B completes Lk but not Lk+1" is exactly the agentic ceiling we're mapping. Exit is non-zero only on a runner error, never on a low score. Recorded honestly either way.

## Confidence
`clean` — a small additive sweep over an existing, just-validated per-model bench; the refactor is pinned by the mock regression tests; the live fleet run is an honest measurement with every outcome valid and informative.
