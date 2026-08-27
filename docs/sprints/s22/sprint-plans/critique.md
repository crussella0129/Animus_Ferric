# Plan Critique — Sprint 22

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: The nudge might not help the 1B at all
- **Failure mode:** outcome-uncertain
- **Response:** **both outcomes are valid + recorded honestly.** Diagnosis (repeat-not-terminate) is grounded in the trace regardless. If the sharper nudge lifts the 1B's measured_level → ship it; if not → ADR-031 documents the real ceiling with evidence. The change is strictly safer wording (it can't regress larger models, which already terminate before the 2nd repeat).

### C-002: Changing the nudge could regress capable models
- **Failure mode:** regression
- **Response:** **no behavior change, only wording.** The two-strike guard and `["warned","stopped"]` sequence are untouched (asserted). Capable models rarely hit even the first repeat (qwen/llama-8b passed L0 in 2 turns); the nudge only fires on a repeat, where a sharper directive is strictly better.

### C-003: Is a wording tweak a "real" sprint?
- **Failure mode:** thin-sprint
- **Response:** the value is the **diagnosis + the measurement**, not the diff size. It pins *why* a 100%-reliable tool-caller fails as an agent (the s21 open question) and tests a concrete hypothesis end-to-end on a real model. The user also framed this as a workflow-test sprint (first one-PR-per-sprint) — a small, clean, well-scoped change is exactly right.

### C-004: One-PR-per-sprint mechanics
- **Failure mode:** process-miss
- **Response:** `main` = s21 (PR #7 merged), `dev` ff'd to match, so `origin/main..dev` will be exactly s22's commits → a clean sprint-22-only PR at close. The close section makes PR creation the explicit final loop-phase step.

## Confidence
`clean` — a grounded diagnosis, a minimal safer-wording change with an unchanged guard contract + updated test, and a live re-bench whose every outcome is a valid, honest result.
