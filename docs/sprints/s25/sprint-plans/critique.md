# Plan Critique — Sprint 25

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Big download + maybe a llama.cpp update
- **Failure mode:** flaky-E2E
- **Response:** floor + ceiling separated. The research verdict (Gemma 4 E4B is usable — official ungated GGUF + mmproj, 4B + function-calling + multimodal) + ADR-035 + docs are the floor and land regardless. The live bench/multimodal are the ceiling; on any load failure they defer with the exact path. User approved the 6 GB download.

### C-002: Pivot away from the approved `--chat` sprint
- **Failure mode:** scope-thrash
- **Response:** the user explicitly redirected — a capable ~4B model is the *right* answer to ADR-033, not a `--chat` workaround for unusably-small models. `--chat` is recorded as a deferred/optional backlog item, not lost.

### C-003: Validation sprint with no Ferric code change
- **Failure mode:** thin-sprint
- **Response:** same shape as s23 (llama.cpp) and s24 (multimodal) — both high-value validation sprints with no harness change. The deliverable is the *evidence*: a real `measured_level` for ~4B (the floor question) + multimodal-under-constraint working, plus naming the reference model. That's substantial.

### C-004: Is ~4B really the floor, or model-specific?
- **Failure mode:** overclaim
- **Response:** the bench gives a *measurement*, not a proof. The ADR frames it as "Gemma 4 E4B clears X" + "our fleet shows 1B none / 8B 5 / 7B 6", so ~4B is where it becomes usable *for these models* — stated as observed, not universal law.

### C-005: One-PR-per-sprint cadence
- **Failure mode:** process-miss
- **Response:** `dev` clean (PR #10 merged). Close: push visible (no `-q`), verify `origin/main..dev` = s25 only, verify PR count — per [[one-pr-per-sprint]].

## Confidence
`clean` floor (research + ADR + docs); the live Gemma 4 run is a user-approved spike whose every outcome (clears the floor / partial / load-deferred) is recorded honestly.
