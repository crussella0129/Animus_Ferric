# Finalized - DO NOT EDIT

# Sprint 91 — Build Plan

- [x] T-1 — Bring the Docker daemon up; confirm engine + available runtimes.
- [x] T-2 — Write availability-gated live sandbox tests (skip loudly, never fail CI).
- [x] T-3 — Validate the airlock: isolation, explicit egress, capability drop, fail-closed default.
- [x] T-4 — Bound `check_available()` after hitting its ~60 s hang.
- [x] T-5 — Determine whether Docker unblocks D2 (it does not — the proxy does).
- [x] T-6 — ADR-081, README, agent-tasks, sprint artifacts.

## Out of scope

Building the allowlist proxy, wiring D2 behind it, fleet re-calibration, C7/C8/B1.
