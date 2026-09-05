# INT-0006 — Truthful policy contract

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0006
- **State:** proposed
- **Work evidence:** [T-11406 backlog](../work/tasks.md#book-v2-carry-forward-from-sprint-113); [T-11905 next repository-wide review/refactor](../work/tasks.md#next-sprint--repository-wide-review-and-bounded-refactor)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 113 gap audit](../sprints/s113/sprint-research/research-report.md)

## Intent

Make every public run-policy field and capability claim correspond to active,
tested runtime behavior or an explicit reserved/unavailable state. Ferric must
not imply planner or subagent behavior through inert fields, defaults, help,
configuration, or traces.

Configuration is part of that authority contract. Missing optional files may
use documented defaults; unreadable, malformed, or invalid present files must
not silently reveal lower-precedence hooks/skills or change the harness. Human
and machine surfaces must bind effective settings and backend discovery to the
same selected workspace, and reject impossible numeric policy inputs without
exposing credentials in diagnostics.

## Acceptance criteria

1. Every serialized/public `RunPolicy` field has a named runtime consumer and
   behavior test, or is removed through an additive compatibility migration.
2. Planner fields remain unavailable until a real protocol exists; no label or
   budget silently routes to Evidence-only execution.
3. `allows_subagents` is either wired to an explicit ICM authority boundary or
   removed/reserved without suggesting autonomous delegation.
4. CLI help, configuration reference, traces, debug output, and tier snapshots
   expose the same availability truth.
5. Literal old policy/config fixtures retain safe defaults and unknown or
   ambiguous new values fail closed.
6. Present invalid configuration fails before provider use, mutation, or hook
   execution; absence alone is a defaultable condition. Precedence and explicit
   workspace selection are consistent across supported surfaces, and effective
   settings (including streaming) describe actual behavior. Diagnostics identify
   the setting/file and a safe corrective action without printing secrets.

## Rationale

The wider-field audit found `uses_planner`, plan budgets, and
`allows_subagents` populated without matching runtime behavior. Sprint 113
correctly rejected EvidencePlanner, making the remaining inert vocabulary a
truthfulness and maintenance problem even when it does not yet cause execution.

## Alternatives

- Leave fields as undocumented future placeholders: rejected because public
  serialized state is already a contract.
- Implement a planner merely to justify the fields: rejected; availability
  must follow evidence, not schema pressure.
- Change only documentation: insufficient if wire/debug state still implies
  behavior.

## Consequences

Some old serialized shapes may need compatibility aliases or deprecation notes.
The policy surface becomes smaller and more honest, and later orchestration
must earn a new explicit protocol.

## Transition history

- 2026-09-05: expanded the proposed intent's configuration boundary and added
  AC-6 after the Sprint 120 review reproduced silent layered-config fallback
  and identified workspace/streaming inconsistencies. This is a semantic
  clarification, not a claim that the inert-field audit or refactor is complete.

- 2026-08-26: created as `proposed` from the Sprint 113 dead-policy-field audit.
