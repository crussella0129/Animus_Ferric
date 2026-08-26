# Plan Critique — Sprint 23

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: The live install may fail autonomously
- **Failure mode:** flaky-E2E
- **Response:** **floor + ceiling separated.** The launcher contract test + ADR-032 + docs are the AI-verifiable floor (land regardless). The live A/B is upside; on failure it's deferred to the user with exact, documented steps (the ollama-blob trick included). User explicitly approved the attempt.

### C-002: Is this "new architecture" or busywork?
- **Failure mode:** redundant-work
- **Response:** **validation of a never-run path.** Ferric *defaults* to llama-server but has only ever been driven against ollama — so the constrained valve on llama.cpp is unproven. Proving it (and documenting the wide-context + edge story the user is after) is real, and it unblocks the multimodal engine + edge targets.

### C-003: ollama-blob-as-GGUF might not load
- **Failure mode:** assumption
- **Response:** ollama stores the model layer as a raw GGUF (mediaType `…image.model`); `llama-server -m <blob>` reads GGUF by content, not extension. If a specific blob fails, fall back to a tiny GGUF fetch — but the no-download path is tried first and is the documented trick.

### C-004: Touching the engine could regress the ollama path
- **Failure mode:** regression
- **Response:** **no behavior change to the launcher** — T-2301 only *adds tests* to `command()` (a pure fn); the ollama arm is asserted unchanged. Docs reframe defaults already in code. Any code fix is gated on the live run actually exposing one.

### C-005: PR cadence (one PR per sprint)
- **Failure mode:** process-miss
- **Response:** PR #8 (s22) is open on `dev`; flagged to the user. A clean s23 PR needs #8 merged first, else the diff bundles 22+23 — handled at close per [[one-pr-per-sprint]] (confirm `origin/main..dev` = s23 only).

## Confidence
`clean` for the floor (pure-fn tests + ADR + docs); the live A/B is an honest, user-approved spike whose every outcome (works / documented-deferred) is recorded.
