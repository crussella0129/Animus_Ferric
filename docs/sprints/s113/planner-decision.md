# Sprint 113 Planner Decision

## Decision

Reject an `evidence_planner` implementation for Sprint 113. The policy remains
unavailable and must fail closed; no product surface may relabel an
evidence-only run as planner execution or silently fall back after a planner
request.

## Evidence basis

The [frozen development screen](sprint-tests/development-screen.md) produced
three complete, infrastructure-clean, structurally verified rows for each
retained candidate. The unchanged mechanism and both permitted general
revisions each completed 0/3 objectives and contracts. The final candidate
therefore missed the minimum 1/3 selection threshold and exhausted the
revision budget.

Because there was no selected candidate hash, the
[paired-confirmation audit](sprint-tests/confirmation-skip.md) records the
required 18-row experiment as deliberately skipped. The
[held-task and teardown audit](sprint-tests/held-and-teardown.md) likewise
records no held evaluation, preserves the remaining held-task boundary, and
proves the managed server is down. The tracked hashes and trace locations are
indexed by the [artifact archive](control-artifacts/artifact-archive.md).

## Rationale

The approved order required the evidence-only mechanism to demonstrate at
least one objective-and-contract completion before adding planner state. It did
not. A planner arm at this point would add unvalidated orchestration and trace
transitions on top of an executor that never cleared its minimum outcome gate;
there is no frozen evidence candidate against which planner value could be
measured. Implementing or informally substituting it would therefore defeat the
experiment's causal boundary rather than answer the planner question.

## Availability contract

- No planner protocol, transition state, linked trace session, or planner arm
  is implemented by this decision.
- Existing fail-closed `evidence_planner` preflight behavior remains the
  authoritative product behavior.
- Evidence-only execution remains explicitly labelled as evidence-only.
- No planner episode was run and no planner performance claim is made.

Any future planner work requires a new user-approved intent and plan. At a
minimum, that work must define a versioned plan schema, bind observed files and
invariants to execution, link plan and execution provenance, prohibit silent
fallback, and measure a distinct planner arm against a newly qualified
evidence-only candidate.
