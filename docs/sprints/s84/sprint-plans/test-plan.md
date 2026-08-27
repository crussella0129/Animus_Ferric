# Finalized - DO NOT EDIT

# Sprint 84 — Test Plan

| Item | Must now hold |
|---|---|
| A4 | a poisoned lock is recovered, not fatal; `send_input` errors (not panics) off a multi-thread runtime, and succeeds on one; stdin survives a second write |
| A5 | the default emits `--network none`, never `--network bridge`; gVisor required; caps always dropped |
| A7 | approval runs the tool; rejection denies and the handler never runs; no approver denies with a reason; **an untainted call never reaches the approver** |
| C1–C5 | no behaviour change — the existing suite is the test |
| Dark Matter | a DM-shaped `{"target": …}` call works; `target` scopes to one corpus; a capped result says so; an uncapped one does not |

The A7 "untainted call never reaches the approver" case is the important one:
without it, this would prompt a human on every write, which is how a security
control gets switched off in practice.

## Gate

`cargo test --workspace` (expect > 487), clippy 0 warnings, fmt clean, and DM's
`verify-spec.sh` green **with its new check verified by negative control**.
