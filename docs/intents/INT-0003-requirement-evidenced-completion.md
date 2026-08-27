# INT-0003 — Requirement-evidenced completion

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0003
- **State:** proposed
- **Work evidence:** [T-11402 backlog](../work/tasks.md#book-v2-carry-forward-from-sprint-113)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 113 gap audit](../sprints/s113/sprint-research/research-report.md)

## Intent

Give Ferric a typed requirement ledger so `task_complete` means the requested
obligations have evidence, not merely that tools stopped failing. This is a
controller capability, not a hidden planner: it must preserve the user's
requirements and causal evidence without inventing subtasks or task-specific
prompt patches.

## Acceptance criteria

1. A session records stable requirement identifiers and exact source text
   before action, with explicit `unaddressed`, `claimed`, and `evidenced`
   transitions.
2. Claims cite concrete file effects, observations, or named-check outcomes;
   model prose alone cannot move a requirement to `evidenced`.
3. `task_complete` fails closed while a required item is unaddressed or its
   evidence is stale after mutation, while optional/advisory items remain
   distinguishable.
4. Trace validation, replay, clarification, resume-of-resume, and compaction
   reconstruct the same ledger independently of model-authored summaries.
5. Dense multi-file benchmark tasks compare objective and contract completion
   against Legacy and the abandoned Evidence intervention without changing
   graders, model, or task prompts.

## Rationale

Sprint 113 made workspace facts causal but still gave the controller no model
of the user's obligations. The frozen candidate could obey file/check barriers
and still finish 0/3. The supplied analyses' contract-density diagnosis remains
plausible and untested; a typed ledger is the narrow experiment that tests it.

## Alternatives

- Revive EvidencePlanner: rejected until a smaller requirement-state protocol
  proves value; the previous planner arm was explicitly abandoned.
- Prompt the model to maintain a checklist: rejected because replay and
  compaction could rewrite it and completion would still trust prose.
- Infer success only from tests: insufficient for non-executable requirements.

## Consequences

Traces and completion gates become larger and stricter. Requirement extraction
errors become a new failure mode, so declarations must remain inspectable and
must never silently narrow the user's request.

## Transition history

- 2026-08-26: created as `proposed` after Sprint 113 falsified file-evidence alone.
