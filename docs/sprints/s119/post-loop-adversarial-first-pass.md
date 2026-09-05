# Sprint 119 extra post-Loop audit — first pass

**Independent read-only verdict: block.** Reviewed closed head `f3cb48b` and
source `81c9aea`, before any Sprint 119 PR. Reviewer was a fresh agent that did
not implement the sprint. No code, tests, branches or remote state were changed
by this review.

## Blocking finding

**P2 / E01:** `crates/ferric-process/src/unix.rs` at the reviewed source returns
success for a removed registry key (line 124) or breaks after confirmed absence
(line 151) before checking the deadline (line 158). Registry contention,
reaping work or scheduler delay can certify an expired observation. Both
paths need deadline-first acceptance and a deterministic regression. Root's
adjacent inspection found the same ordering in Windows suspended-child rollback
(E02), which belongs in the same bounded correction.

## Actual phase audit

| Phase | Independent verdict |
|-------|---------------------|
| Research | Evidenced: committed survey, sources, findings, scope limits and preservation authorization precede Build. |
| Plan | Evidenced: independent concerns resolved; plans locked at `b46fba8` and unchanged since. |
| Build | Evidenced: three distinct task commits with reachable ledger references; corrections stay within locked scope. |
| Test | Execution evidenced, acceptance blocked: all six CI jobs independently confirmed successful at exact source `81c9aea`, but E01 has the uncovered defect above. |
| Loop | Reconciliation and actual close exist; acceptance and closure must be explicitly superseded after this failed audit. |

At inspection all 16 commits above `origin/main` belonged to Sprint 119, and
changes after the tested source were Book-only. GitHub had no open dev-to-main
PR, preserving audit-before-PR order. Positive findings included shared source
ownership, scoped Linux reaping, explicit platform limitations, retained failed
attempts, truthful active intent, and Cargo-based non-root namespace CI.

This was not an exhaustive repository review or independent reproduction of
historical local tests. Final remote receipts and exact user-stash restoration
remain pending. The repository-wide review/refactor stays next-sprint work.
