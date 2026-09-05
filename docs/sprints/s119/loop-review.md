# Sprint 119 Loop reconciliation

## Scope and intent

INT-0008 remains active. The accepted source-process safety increment does not
complete its compact workflow, model-backed application trial, broad platform
parity, ordinary-host Linux ownership, or live tailnet acceptance. Existing
ordered tasks remain queued. T-11904 retains broader Linux owner-kill/group
escape containment; T-11905 makes the requested repository-wide review/refactor
the next sprint after owner merge and points to its read-only preparation.
INT-0006 stays proposed until that sprint's Research/Plan select its work.

## Phase receipts for the extra adversarial audit

| Phase | Actual evidence |
|-------|-----------------|
| Research | `e720128`, `5584be9`, revised INT-0008 AC-6, 20-file/four-primary-source report; clean-state gate passed after the one explicitly authorized unrelated-file stash. The budget helper was rechecked at Loop preparation. |
| Plan | First independent critique found three gaps; revised clean critique precedes finalization. `b46fba8` records locked plans and queued tasks. The Codex host's missing Claude Plan Mode tools are disclosed; source-freeze and explicit owner acceptance were preserved. |
| Build | T-11901 `a18d1a3`, T-11902 `4e9aed9`, T-11903 `92e8f29`, each with reachable commit-evidence backfill; source checks at coherent boundaries. Scope corrections stayed within locked E01/E04/E05/E07. |
| Test | Final source `81c9aea`, six-job CI `33935893263` success, local 1,128 Windows passes / six intentional ignores, Linux lifecycle 6/6 and Windows 5/5, clause map and independent clean Test critique; `f140d61` commits accepted Test artifacts. Failed attempts and exact corrections remain recorded. |
| Loop | Intent remains truthful/active, durable backlog and next-sprint request retained, optional confidence records a patched outcome, Book validation and terminal close must pass before the extra independent audit and remote checkpoint. |

## Remote and preservation boundary

The source head was pushed and independently matched with `git ls-remote`.
`origin/main..dev` contains only Sprint 119 commits; no sprint branch/worktree
was created. No dev-to-main PR was open at the pre-check. After closure and the
extra independent post-Loop audit, the installed remote adapter must create
exactly one dev-to-main PR, retain its checkpoint URL, and leave merge to the
owner. Actual push/head/base/count and current CI must be verified before
handoff; these are not preclaimed as completed by this preparation record.

The exact user-authorized stash is recorded in sprint metadata. Restore it and
verify the original SHA-256 before handoff, without staging it or including it
in the PR. A clean Book gate run before that restoration is distinct from the
deliberately restored user-owned modification; do not claim a clean working
tree after restoring it. No manual test-process termination, model deletion,
or direct target executable launch is part of this sprint's acceptance.
