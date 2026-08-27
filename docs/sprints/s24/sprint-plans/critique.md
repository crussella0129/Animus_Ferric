# Plan Critique — Sprint 24

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: The vision model might not load / behave in `b9821`
- **Failure mode:** flaky-E2E
- **Response:** floor + ceiling separated. The multimodal mapping is already unit-tested; a trace assertion (image left as a content-part) is AI-verifiable without a good caption. SmolVLM-500M is a llama.cpp-supported model; fallback moondream2. If all fail, the live model run is deferred with exact steps — the *download/model* is the only gap, not Ferric's code.

### C-002: Constraint (grammar) vs a vision model
- **Failure mode:** protocol-conflict
- **Response:** the image is *input*; the grammar constrains *output*. The "describe then task_complete" framing keeps it agentic. If grammar garbles a small VLM, re-run `--protocol native` and record which works — a real finding either way, and it doesn't change the pipeline result (the image still reaches the model).

### C-003: Is this a "real" sprint if no Ferric code changes?
- **Failure mode:** thin-sprint
- **Response:** it **closes the longest-deferred goal** (multimodal, queued since s10) and is the user's stated priority. Validation of an unproven E2E path + an ADR + docs is a complete unit — same shape as sprint 23 (which validated llama.cpp with no launcher change). A code fix lands only if the live run exposes one.

### C-004: Soft (human) part of the verification
- **Failure mode:** unverifiable
- **Response:** split honestly. "Image reached a seeing model" (response references colour/shape) is AI-verifiable; exact caption fidelity is the soft human bit. The heartbeat claim rests only on the verifiable part.

### C-005: One-PR-per-sprint cadence
- **Failure mode:** process-miss
- **Response:** `dev` is clean (PR #9 merged → main = s23). At close: push with visible confirm, verify `origin/main..dev` = s24 only, verify PR commit count — per [[one-pr-per-sprint]].

## Confidence
`clean` — pipeline already tested; the live run is a user-approved spike whose verifiable core (pixels reach a seeing model) is the claim, with an honest deferral path.
