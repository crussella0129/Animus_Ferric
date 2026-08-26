# Finalized - DO NOT EDIT

# Sprint 95 — Build Plan

- [x] T-1 — Calibrate `qwen2.5-coder-3b` over the full L0–L6 ladder.
- [x] T-2 — Re-calibrate `qwen2.5-coder-7b`; compare against its sprint-20 figure.
- [x] T-3 — Investigate the non-monotonic L5/L6 result (repeat runs).
- [x] T-4 — H1: write profiles only from a full ladder.
- [x] T-5 — H2: report levels that failed below the highest pass; 4 unit tests.
- [x] T-6 — Restore the 7B profile; ADR-086, README, agent-tasks, artifacts.

## Out of scope

Multi-sample calibration (running each level N times and reporting a rate) —
noted as the real answer to H2's root cause, but a bigger change than reporting.
