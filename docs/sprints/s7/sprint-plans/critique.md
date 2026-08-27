# Plan Critique — Sprint 7

> Self-critique (no subagent spawned this run) against `prompts/plan-critic.md`'s
> seven failure modes. Each concern has an inline **Response**.

## Concerns

### C-001: T-008 doc-assertion EARS clauses have no matching `test_*`
- **Where:** `build-plan.md` T-008 success criteria; `test-plan.md` (no Unit/Integration entry for T-008).
- **Quote:** "WHEN `decisions.md` is read, THEN it SHALL contain ADR-021 and ADR-022."
- **Failure mode:** plan-test-mismatch
- **Why it matters:** every EARS clause should be traceable to a check; doc clauses can silently rot.
- **Response:** **defer-with-rationale** — these are documentation-content assertions, not code behavior. Verified by a `grep` doc-check in the Test phase, not a unit test. **Amended `test-plan.md`** with an explicit Test-phase doc-check line so the clause is traceable.

### C-002: ConstrainedJson needs tool descriptions in the system prompt (the schema is NOT injected)
- **Where:** `build-plan.md` T-004; flagged as a risk in `research-report.md` §4 ("schema-not-in-prompt").
- **Quote (research):** "llama.cpp constrains output to the schema but does not show the model the schema."
- **Failure mode:** missing-risk / hidden-dep
- **Why it matters:** the constraint guarantees *structure*, not *semantics*. In ConstrainedJson mode `tools` is empty, so unless the composed system prompt enumerates the tools + their input schemas, the model cannot choose the right tool/args even though its output is forced to be valid JSON.
- **Response:** **fix-in-plan** — **amended T-004 notes** to require the ConstrainedJson path to use the composed (`ferric-prompt`) system prompt enumerating available tools and their schemas (not the bare `DEFAULT_SYSTEM_PROMPT`), and to verify `ferric-prompt` already emits tool descriptions during build. The E2E capability probe (E2E-1) is the backstop.

### C-003: `research-report.md` Decisions Reviewed omits two touched ADRs
- **Where:** `research-report.md` `## Decisions Reviewed`; build-plan touches `scale.rs` (T-004) and realizes the HTTP-valve wrapper (T-002).
- **Failure mode:** ignored-ADR
- **Why it matters:** **ADR-017** ("HTTP escape-valve … shared `validated_complete` wrapper that makes ADR-010 backend-boundary enforcement model-free testable") is *directly realized* by this sprint and was not listed; **ADR-006** (scale function purity) owns `scale.rs`, which T-004 edits (only the `ActionProtocol` enum, not `policy_for`).
- **Response:** **fix-in-plan** — **amended `research-report.md`** Decisions Reviewed to add ADR-017 (realized) and ADR-006 (touched, invariant unaffected — `policy_for`/the table are untouched).

### C-004: T-004 spans five files and four EARS clauses — granularity
- **Where:** `build-plan.md` T-004 "Replace the protocol dichotomy … and wire it through the loop."
- **Failure mode:** granularity
- **Why it matters:** elementary tasks are "one coherent diff"; T-004 is the heaviest task.
- **Response:** **reject (with rationale)** — the `ActionProtocol` rename cannot be split into compiling commits: renaming the enum in `scale.rs` immediately breaks `run.rs`/`protocol.rs`/`query.rs`, so the change must land atomically to keep the workspace green. It is one logical concern (the protocol trichotomy). Accepted as a single coherent (if large) diff; the four EARS clauses are distinct behavioral surfaces of that one concern, which the schema explicitly permits.

### C-005: T-001 under-specifies the literal-update fan-out
- **Where:** `build-plan.md` T-001 Touches.
- **Failure mode:** hidden-dep (compile breakage)
- **Why it matters:** adding `supports_constraint` to `Capabilities` and `constraint` to `CompletionRequest` forces **every** struct literal of those types to update, or the workspace won't compile — including `Capabilities {}` in `mistralrs.rs`, `openai.rs`, `mock.rs`, and test helpers (`ferric-loop/tests/common/mod.rs`).
- **Response:** **fix-in-plan** — **amended T-001 notes** to call out that all `Capabilities { .. }` and `CompletionRequest { .. }` literals across the workspace must be updated in the same diff (the compiler enumerates them).

### C-006: T-005 `Depends on: (none)` shares `lib.rs` with T-001 and T-008
- **Where:** `build-plan.md` T-005 vs T-001/T-008 (all touch `crates/ferric-provider/src/lib.rs`).
- **Failure mode:** hidden-dep
- **Why it matters:** overlapping `Touches` can imply a missing dependency edge.
- **Response:** **reject (with rationale)** — the dependency is *logical*, and removing the Python backend needs nothing from the constraint work. The three tasks edit disjoint regions of `lib.rs` (T-001 adds a `Constraint` export, T-005 removes the `python` mod lines, T-008 fixes the module doc-comment) and run in sequence T-001 → T-005 → T-008, so there is no conflict. `(none)` is correct for logical dependency.

## Confidence
`proceed-with-caveats` — no concern is severe enough to block. The two substantive fixes (C-002 prompt-enumerates-tools, C-005 literal fan-out) and the two record fixes (C-001 doc-check, C-003 ADR coverage) are applied to the plan/research artifacts below before lock; C-004 and C-006 are rejected with rationale.
