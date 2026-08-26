# Finalized - DO NOT EDIT

# Sprint 87 — Test Plan

## The guard must

| Case | Expected |
|---|---|
| A-B-A-B for 8 turns | `Stop` with reason `oscillation`, after a warning |
| Alternating names, fresh args | **Proceed** — the false-positive boundary |
| Identical repeats | still `repetition` (don't steal the sharper guard's case) |
| 3-cycle | not caught (documented decision) |
| Empty turns | ignored — they are the no-action-nudge path |

## Live

Re-run the exact sprint-86 prompt and require the stop reason to change from
`max_turns` to `oscillation`. Force A1's cap with a single-long-line file and
check the trace keeps the full text while the prompt does not.

## Gate

`cargo test --workspace` > 507, clippy 0, fmt clean.
