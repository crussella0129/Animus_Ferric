# Finalized - DO NOT EDIT

# Sprint 94 — Build Plan

- [x] T-1 — `--research-url` (repeatable) + `--allow-standard-runtime`.
- [x] T-2 — `url_host`: strip userinfo/port, validate before the airlock opens.
- [x] T-3 — Wire the web plane into `drive_real`: one airlock per run, RAII.
- [x] T-4 — Contaminate the run on ingest; fail loud on a fetch error.
- [x] T-5 — Unit tests for the boundary; live end-to-end verification.
- [x] T-6 — ADR-085, README, agent-tasks, sprint artifacts.

## Out of scope

Prebuilt gateway image (~15 s startup), fleet re-calibration, C7/C8/B1.
