# Finalized - DO NOT EDIT

# Sprint 90 — Build Plan

- [x] T-1 — `ferric_guard::Provenance`; `SinkPolicy::decide` takes it; delete `TaintSet`.
- [x] T-2 — Thread through `Registry::execute`, `RunArgs`, `dispatch` (~40 sites).
- [x] T-3 — CLI: stamp `UntrustedIngested` where digests are ingested; default `--sink-action` to `requireapproval`.
- [x] T-4 — Unit + integration tests for both halves (clean unaffected, contaminated gated).
- [x] T-5 — Validate live: clean / contaminated-no-approver / contaminated-approved.
- [x] T-6 — ADR-080, README, agent-tasks, sprint artifacts.

## Out of scope

Fleet re-calibration, A5's sandbox (Docker), C7/C8/B1.
