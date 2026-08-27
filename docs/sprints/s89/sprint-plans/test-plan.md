# Finalized - DO NOT EDIT

# Sprint 89 — Test Plan

| Case | Expected |
|---|---|
| accept-edits + RequireApproval + tainted write | **exactly one** prompt |
| the approved call | **runs** — not suppressed-then-denied |
| no approver + RequireApproval | still denies; nothing touches disk |
| tainted preview | carries the sink's warning |
| untainted preview | carries no warning |

The second row is the one that distinguishes a real fix from a prompt-count
cosmetic.

## Gate

`cargo test --workspace` > 524, clippy 0, fmt clean.
