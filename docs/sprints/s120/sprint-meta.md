# Sprint 120 Meta

- **Sprint number:** 120
- **Book schema version:** 2
- **Start timestamp:** 2026-09-05T02:20:09Z
- **End timestamp:** (filled at Loop Phase)
- **Model:** gpt-6-astra, ultra (user-selected; runtime identity not independently exposed)
- **Bundle version:** 0.22.0
- **Exit status:** in-progress
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Human-first prepared-host startup and repository review; Research complete, reviewed Plan proposal awaits owner approval.
- **Intents:** [INT-0008](../../intents/INT-0008-unified-local-model-workflow.md), [INT-0006](../../intents/INT-0006-truthful-policy-contract.md), [INT-0005](../../intents/INT-0005-safe-multilanguage-syntax-admission.md); [INT-0007](../../intents/INT-0007-hardware-calibrated-autonomous-development.md) reviewed dependency, not an acceptance claim.
- **Completion evidence:** (filled at Loop Phase)

## Intake and preservation

PR 107 was verified merged at `2026-09-05T02:10:13Z`, merge commit
`17fc166bc8143ef85f3f3859f6a156902e0a68dd`. Main and dev were synchronized to
that baseline before initializing Sprint 120. The baseline also includes the
owner-merged RustPython dependency PR 106; its compile failure is R01, not a
Sprint 120 source regression. No feature branch or worktree was created.

The unchanged user edit at
`docs/sprints/s114/control-artifacts/model/acquisition-tests.json` is preserved
in file-specific stash `5821f481e133a45c0a41a77fbc7c575df62f2ce3`.
Original SHA-256:
`8ecf94878e7ad745aea28a9365af58ee111c80b26d21a15a0f434edb2beb75db`.
Restore that exact edit and verify the hash before handoff; never stage it into
this sprint. Retain the stash as recovery evidence.

The requested 0.21.0 skill path is absent; installed 0.22.0 is used and was
disclosed. Its Claude-specific EnterPlanMode/ExitPlanMode APIs are unavailable
on this Codex host. Source remains unchanged during Research and proposal
review; no plan approval or Build completion is inferred from tool absence.

## Research exit and Plan checkpoint

Research/intent records were committed at `9368bff`. The installed budget helper
reported `files=51 sources=5` (file cap exceeded); the report's explicit Budget
Override justifies the owner-requested cross-crate review. The clean Book check
passed and the router reported `plan`. No source implementation was changed.

[Plan scratch](sprint-plans/plan-proposal.md) contains the proposed Build/Test
plans and exact affected acceptance boundaries. Its
[independent preliminary critique](sprint-plans/proposal-critique.md) is clean
after correcting an overbroad cancellation acceptance implication. The proposed
provider task selects R04/R16 from the initial review follow-up list; other
unselected findings remain durable backlog. Canonical build/test plans remain
empty and unlocked; this preliminary review does not substitute for their
required post-approval critic and lock.

The installed skill's explicit plan-approval gate pauses this turn. Approval
has not been inferred. Research is complete, Plan is awaiting owner approval,
and Build, Test, Loop, the extra post-Loop audit and the sprint PR have not
occurred. Sprint status remains in-progress. The user's one-sprint/one-PR rule
is preserved: no second sprint or premature PR is created.
