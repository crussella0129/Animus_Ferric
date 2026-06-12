# Plan Critique — Sprint 1

> Critic: subagent (adversarial review per prompts/plan-critic.md), 2026-06-11.
> Primary-agent dispositions inline as **Response:**.

## Concerns

### C-001: ProviderError missing RetryableBackend variant
- **Failure mode:** EARS-vague / missing-risk
- **Response: FIXED-IN-PLAN.** T-103 retitled and amended: `RetryableBackend(String)` is explicitly a NEW variant added in T-103, with semantics locked (Backend = permanent, RetryableBackend = transient) and a full truth table in the EARS.

### C-002: CheckRecord type undefined
- **Failure mode:** EARS-vague / missing-risk
- **Response: FIXED-IN-PLAN.** T-102 notes now define `CheckRecord` (registry.rs) and specify that `ExecuteOutcome::Completed/Denied` gain `checks: Vec<CheckRecord>`.

### C-003: Event field schemas underspecified
- **Failure mode:** EARS-vague
- **Response: FIXED-IN-PLAN.** T-101 notes now lock the field schema for all six new events.

### C-004: L0 smoke session_end reason assertion too narrow
- **Failure mode:** e2e-drift
- **Response: REJECT.** The narrowness is the point: L0 is the lineage's capability gate ("single tool op works, cleanly terminated"). Accepting `max_turns`/`repetition_guard` would mark a failing model run as passing — exactly the false-green the real-GGUF policy (ADR-009) exists to prevent. The prompt is engineered for task_complete, sampling is deterministic (reproducible), and a narrow-gate failure is a *finding to surface*, not flake to tolerate.

### C-005: clap dependency not formally amended per ADR-004
- **Failure mode:** ignored-ADR
- **Response: FIXED-IN-PLAN.** T-113 now records an explicit ADR-004 allowlist amendment (mistralrs feature-gated, tokio feature-gated, clap unconditional, futures-executor promotion), preserving the aarch64-gate invariant.

### C-006: validate() timing unclear (loop vs backend)
- **Failure mode:** EARS-vague / hidden-dep
- **Response: FIXED-IN-PLAN.** T-103 + T-104 notes: the loop calls `request.validate()` before every `provider.complete()` (primary enforcement); backends validate again at their boundary (defense in depth). The existing `adr010_request_shape` integration test asserts the loop never builds an invalid shape.

### C-007: Retryability scope per provider undefined
- **Failure mode:** EARS-vague
- **Response: FIXED-IN-PLAN.** T-103 EARS now enumerates the truth table; T-109 notes classify mistralrs errors (transient → RetryableBackend; load/GGUF/template → Backend).

### C-008: T-104 file list vs T-105..107 module structure
- **Failure mode:** granularity
- **Response: FIXED-IN-PLAN.** T-104 notes state it is the core scaffold; T-105..T-107 add their modules and extend run.rs (their Depends-on lines express the relationship).

### C-009: Executor boundaries (mock vs real vs smoke) unclear
- **Failure mode:** EARS-vague
- **Response: FIXED-IN-PLAN.** T-111 notes now specify: --mock on futures_executor (no tokio in default build), real path on a tokio multi-thread Runtime, L0 smoke spawns the binary as a separate process to avoid executor contention.

## Confidence

`proceed-with-caveats` (critic) → 8 fixed in plan, 1 rejected with rationale (C-004 — the narrow gate is intentional). Plans amended and ready to lock.
