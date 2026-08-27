# Plan Critique — Sprint 15

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Cap-only — can't raise rings above tier
- **Failure mode:** under-power vs the literal "control exactly"
- **Response:** **deliberate + safe.** Raising a weak model into rings it can't drive would tank reliability (the whole thesis). Expansion stays *earned* via `measured_level` (ADR-019) — which is itself explicit config, so you CAN run a small model with more rings by declaring/measuring it. `--max-ring` is the restrict knob; together they give full control without a footgun. Documented.

### C-002: Adding a `RunPolicy` field
- **Failure mode:** blast-radius
- **Response:** **3 compiler-enumerated sites** (`policy_for` + 2 test helpers). The snapshot test is field-assert style (doesn't construct `RunPolicy`, doesn't reference the new field) → untouched. Low risk.

### C-003: `u8::MAX` sentinel for "no cap"
- **Failure mode:** magic-value
- **Response:** **fine.** `max_ring.unwrap_or(u8::MAX)` inside a `min` is the idiomatic "None ⇒ unbounded"; rings are tiny so there's no overflow concern, and an over-large `--max-ring` correctly no-ops (capped by tier).

### C-004: testing via the trace's `offered_tools`
- **Failure mode:** weak-assertion / coupling
- **Response:** **accept (strong).** `PromptAssembled.offered_tools` is the *actual* tool set fed to the grammar, so asserting it proves the cap end-to-end (CLI→policy→tools_for_policy→grammar) without a model — better than a unit-only check.

## Confidence
`clean` — a small additive field + a `min`, plus a CLI flag; restrict-only by design; proven end-to-end through `--mock` + the trace.
