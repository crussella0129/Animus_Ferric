# Sprint 115 unit and static tests

## T-11414 query surface

- Query unit tests: 35 passed.
- Default CLI integration suite: 68 passed.
- `backend-openai` CLI integration suite: 69 passed.
- Whole Ferric binary suite outside the restricted Windows
  process-inspection sandbox: 176 passed.
- Backend-enabled all-target Clippy with warnings denied, workspace formatting,
  and `git diff --check`: passed.

Coverage includes default and external trace allocation, lexical/canonical
overlap rejection, post-create path replacement, Windows reparse handling,
linked external continuations, and PowerShell/POSIX exact-argv resume hints.

## T-11503 retained-runtime controls

The final static runtime-control suite passed 31 checks and parsed all five
PowerShell scripts. It covers invariant ISO timestamp retention, exact UTC
instant comparison, three- and ten-tick Windows timestamp compatibility,
eleven-tick rejection, exact predecessor-manifest allowlisting, attempt
snapshot immutability, allocation, and fail-closed control behavior.

These static checks did not launch a server or inference request.
