# Finalized - DO NOT EDIT

# Sprint 93 — Build Plan

- [x] T-1 — `Airlock` type: create both networks, run the gateway, expose a policy.
- [x] T-2 — Poll for readiness; detect an exited gateway; fail closed.
- [x] T-3 — RAII teardown via `Drop`.
- [x] T-4 — Validate allowlist entries before touching docker; test the injection cases.
- [x] T-5 — Live tests: enforcement, teardown, and no-debris-on-rejection.
- [x] T-6 — ADR-083, README, agent-tasks, sprint artifacts.

## Out of scope

D2 itself (CLI surface, allowlist as configuration, per-run vs per-query), and
the prebuilt gateway image that would cut the ~15 s startup.
