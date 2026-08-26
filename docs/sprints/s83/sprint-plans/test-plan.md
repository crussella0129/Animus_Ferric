# Finalized - DO NOT EDIT

# Sprint 83 — Test Plan

Each defect keeps the sprint-82 probe that demonstrated it, promoted to a
permanent regression test. A fix is accepted only when its probe flips.

| Defect | Must now pass | Must still hold |
|---|---|---|
| A3 | staged index survives a snapshot | revert round-trip; untracked files still captured |
| A1 | large output reaches the model truncated | the trace still holds the FULL output |
| A2 | a lifted sentence from untrusted text is tainted | short fragments do not become needles |
| A6 | `"Go"` matches an all-Go vault | `"Go"` does NOT match `"algorithm"`; stems still match |

The "must still hold" column is the point: three of these fixes have an obvious
form that breaks the property beside it.

## Gate

`cargo test --workspace` (expect > 463), `cargo clippy --all-targets` (0
warnings), `cargo fmt --check`.
