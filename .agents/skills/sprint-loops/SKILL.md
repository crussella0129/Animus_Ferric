---
name: sprint-loops
description: Compatibility pointer for the installed Sprint Loops Book v2 workflow.
---
# Sprint Loops Book v2 compatibility

This repository does not carry a second workflow implementation. When a user
explicitly asks to start, continue, or resume a sprint loop, load the host's
installed `sprint-loop:sprint-loop` skill and follow its router and phase
contracts.

The tracked Project Book is the durable state machine:

- semantic intent: `docs/intents/`
- active and completed work: `docs/work/`
- sprint provenance: `docs/sprints/`
- navigation only: `docs/SUMMARY.md`

Never create a parallel root-level ledger or infer activation merely because
the Book exists.
