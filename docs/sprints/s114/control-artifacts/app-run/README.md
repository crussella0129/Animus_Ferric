# Sprint 114 app-run evidence

T-11410 stopped at preflight before model inference or candidate mutation.
The calibrated Ferric binary creates `.ferric/trace` inside its workspace,
while the frozen MH-RS01 grader rejects every directory except `src/` and
`tests/`. The required model-visible `run_check` therefore cannot pass during
the query under the frozen boundary.

`capture-preflight.ps1` records the calibrated binary identity, CLI surface,
source bindings, frozen-grader identity, cold execution state, and the exact
collision. It is fail-once: existing evidence is never overwritten.

The grader, seed, checks, calibrated runtime evidence, and candidate workspace
remain unchanged. Resolution requires a separately planned operator-only
external trace root and independent qualification of the rebuilt Ferric
binary; that product change is not part of the frozen T-11410 task.
