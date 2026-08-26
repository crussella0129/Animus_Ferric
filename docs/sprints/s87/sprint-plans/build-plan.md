# Finalized - DO NOT EDIT

# Sprint 87 — Build Plan

- [x] T-1 — New windowed `OscillationGuard`; wire into the loop, `StopReason`, trace event, projector warning, trace renderer.
- [x] T-2 — Unit + integration tests, including the false-positive boundary.
- [x] T-3 — Validate the fix LIVE on the scenario that found the bug.
- [x] T-4 — Validate A1's truncation cap live (forced with a long-line file).
- [x] T-5 — Attempt A2/E2 live via `--research` → surfaced G1 instead.
- [x] T-6 — ADR-077, README, agent-tasks, sprint artifacts.

## Out of scope

G1's fix (root-cause isolation + a non-silent empty result), A5 (needs Docker),
fleet re-calibration, and the weaker second model (share unreachable).
