# Plan Critique — Sprint 34

> Self-critique against `prompts/plan-critic.md` (user-co-designed; no subagent spawn).

## Concerns

### C-001: A primitive with no enforcement is inert
- **Failure mode:** dead-code
- **Response (user-chosen scope):** primitive-first is the deliberate plan — the same pattern as
  the quarantine + retrievers (build the tested unit, wire next). The end-to-end *shape* test
  (tainted digest → tainted write args → `Deny`) proves the policy will gate a real injected write
  once the dispatch chokepoint calls it. It's ready-to-wire, not speculative.

### C-002: Substring taint → false positives
- **Failure mode:** over-gating
- **Response:** accepted + documented. Conservative is the safe direction for a *write* sink (better
  to over-gate a write than under-gate). The three modes soften it: Warn (observe), RequireApproval
  (ask), Deny (block) — the caller tunes strictness. Empty/whitespace tainted strings never match
  (guarded + tested) so a digest with an empty field can't taint everything.

### C-003: Why all three modes now vs. just Deny?
- **Failure mode:** scope
- **Response:** the user's explicit call — all three available so the wiring can pick Deny
  (autonomous), RequireApproval (human-gated), or Warn (observability-first rollout) without a later
  breaking change. The enum + one match arm; trivial cost, real flexibility.

### C-004: New `ferric-guard` dep on `ferric-research`
- **Failure mode:** dependency-cycle / churn
- **Response:** additive; `ferric-guard` (a workspace crate) doesn't depend on `ferric-research`, so
  no cycle. Reusing `PermissionLevel` is correct — the eventual wiring passes each tool's real
  `spec.permission`, so the policy must speak that exact axis.

### C-005: Where does the gate ultimately live?
- **Failure mode:** unclear-integration
- **Response:** documented (deferred) — the `registry.execute` chokepoint, beside the existing
  `check(permission, path)`, returning the same `Denied` outcome the model already understands; the
  `TaintSet` is populated as research digests enter the agent's context (the research→loop wiring).
  This sprint ships the decision function that gate will call.

## Confidence
`clean` — a small, pure, user-co-designed primitive reusing the existing `PermissionLevel` axis,
fully unit-tested incl. the end-to-end gate shape, with the one real risk (substring false
positives) bounded by the three modes + the empty-string guard and documented honestly. Inert
until wired, by design.
