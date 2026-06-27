# Plan Critique — Sprint 28

> Self-critique against `prompts/plan-critic.md` (no subagent spawn — autonomous loop).

## Concerns

### C-001: Is a third guard over-engineering the loop?
- **Failure mode:** guard-sprawl
- **Response (reject):** the three guards target three *orthogonal* signals — identical
  actions (repetition), same-tool-name streak (no-progress), all-results-error streak
  (failure). The failure mode here (different tools, all erroring) is provably uncaught by
  the other two (they reset on signature/name change), and the integration test demonstrates
  it. Each guard is ~40 lines, single-purpose, independently tested. This is the natural
  completion of the family, not sprawl.

### C-002: False positives on a model legitimately hitting errors
- **Failure mode:** over-eager-guard
- **Response (fix-in-plan, reflected):** `STOP_AT=3` consecutive turns where **every**
  dispatched call errored, with a `Warn` nudge at 2. Any single successful call resets the
  streak (mixed turns = progress). Probing that 404s once or twice won't trip it; three
  straight all-error turns is genuinely stuck. `max_turns` remains the backstop.

### C-003: Threshold tighter than the other guards — justified?
- **Failure mode:** inconsistent-tuning
- **Response:** yes, deliberately. A *failing* streak is a stronger stuck-signal than a
  *succeeding-but-non-advancing* one (no-progress, STOP=5) — repeated hard errors rarely
  self-correct past a nudge, so a faster stop saves more compute. Documented in ADR-038; a
  one-line const change if data argues otherwise.

### C-004: Guard placement — must run after dispatch, unlike the others
- **Failure mode:** wiring-bug
- **Response:** correct and intentional — it keys off `is_error` results, which only exist
  after dispatch. Gated on `terminate_with.is_none()` so a turn that ends in `task_complete`
  (even if an earlier call errored) is a success, not a failure-stop. The integration test's
  "success avoids stop" + a terminating-turn path cover this.

### C-005: Enum-exhaustiveness blast radius
- **Failure mode:** compile-break
- **Response:** known from sprint 27 — exactly two exhaustive `Event` matches
  (`tests/common/mod.rs::kinds`, `trace_cmd.rs`) need a `FailureGuard` arm; the compiler
  enforces it. Additive serde variant; unknown tags → `ParsedEvent::Unknown`. No `verify.rs`
  change.

## Confidence
`clean` — small, additive, mirrors two already-shipped primitives (`RepetitionGuard`,
`ProgressGuard`), fully covered by unit + integration on the deterministic scripted harness,
with the one real risk (false positives) bounded by definition + threshold + warn + backstop
and documented honestly.
