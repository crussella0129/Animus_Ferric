# Sprint 120 Meta

- **Sprint number:** 120
- **Book schema version:** 2
- **Start timestamp:** 2026-09-05T02:20:09Z
- **End timestamp:** (filled at Loop Phase)
- **Model:** gpt-6-astra, ultra (user-selected; runtime identity not independently exposed)
- **Bundle version:** 0.22.0
- **Exit status:** in-progress
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** (one-line description of sprint goal, filled after Plan Phase)
- **Intents:** (filled after Plan Phase)
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
