# Finalized - DO NOT EDIT

# Sprint 90 — Test Plan

Both halves matter equally. A gate that is safe but unusable would be no
improvement on the detector it replaces.

| Case | Expected |
|---|---|
| clean run, any `SinkAction` | **never gated** |
| contaminated + no approver | denied; nothing touches disk |
| contaminated + approver | one prompt; the write lands |
| contaminated, opposite call contents | **same** decision — the unevadability property |
| contaminated read | allowed |

## Live

Re-run the sprint-88 case that was *allowed* under substring taint and require it
to be denied now; plus a clean run (must be unaffected) and an approved one (must
land).

## Gate

`cargo test --workspace`, clippy 0, fmt clean.
