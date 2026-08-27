# Finalized - DO NOT EDIT

# Sprint 83 — Build Plan

Remediation of the sprint-82 audit, ordered by consequence. One commit per item.

- [x] T-1 — A3: snapshot under a private `GIT_INDEX_FILE`; add the containment
      guard; confirm before `revert`. Regression tests.
- [x] T-2 — A1: apply truncation in the projector; seed its limit from the
      registry; drop the dead `_for_model`. Tests on both halves of the contract.
- [x] T-3 — A2: taint content not provenance, at matching granularity. Tests.
- [x] T-4 — A6: keep short terms, match them as whole words. Tests.
- [x] T-5 — Vestigial: B3, B4, B6, B7, B8, C6.
- [x] T-6 — ADR-073, README sprint-log entry, backlog reconciliation.

## Out of scope, with reasons

A4 (de-panic `manage_task`), A5 (invert the sandbox airlock), A7 (wire
`RequireApproval` to `EditApprover`), C1 (`run_with_provider(RunArgs)`), and the
Dark Matter `target` contract decision. A5 and A7 touch subsystems that are still
unreachable from the binary (D1/D2) — doing them properly means wiring them in,
which is a sprint, not a patch.
