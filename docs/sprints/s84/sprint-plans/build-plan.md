# Finalized - DO NOT EDIT

# Sprint 84 — Build Plan

- [x] T-1 — A4: de-panic `manage_task` + `task_registry`; C4 removal path; C5 label.
- [x] T-2 — C1: `run_with_provider(RunArgs)` + `LoopSetup`; C2 post_turn; C3 vcs sync.
- [x] T-3 — A7: wire `RequireApproval` to the `EditApprover`.
- [x] T-4 — A5: invert the sandbox airlock; make argv testable without Docker.
- [x] T-5 — Dark Matter: accept `target`, signal truncation, harden DM's verifier.
- [x] T-6 — ADR-074, README entry, backlog reconciliation.

## Out of scope

C7 (`ferric-cli`'s 19 flat modules) and C8 (scattered test scripts) are
organisational and touch no behaviour; B1 (`Protocol`'s dead variants) is a
trace/profile schema change, not a deletion. The DM **return shape** is left open
deliberately — see the research report.
