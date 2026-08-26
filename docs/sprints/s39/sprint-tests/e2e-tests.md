# Sprint 39 E2E Tests

- **Status:** possible (via `--mock`, no real GGUF model required) — and largely covered by
  T-3905's CLI subprocess tests (`integration-tests.md`), which already exercise a real `ferric`
  binary against a real trace file on real disk — the project's established "subprocess + real
  disk" e2e bar, per sprint 38's precedent.
- The single strongest E2E-flavored proof this sprint is actually `ferric-loop::resume_tests::
  real_run_then_replay_then_resume_reaches_task_complete` (test-critic C-010) — a genuine round-trip
  through a REAL `run()` call, a REAL trace file (truncated to simulate a kill), a REAL `replay()`
  call, and a SECOND real `run()` call reaching `TaskComplete`. It's filed under `unit-tests.md`
  (co-located in the `ferric-loop` crate) rather than duplicated here, since it already meets a
  stronger bar than the CLI-level fixtures (no hand-built trace on either side of the boundary it
  tests) — matching sprint 38's precedent of not duplicating a test across files just to satisfy a
  section heading.
- A real interrupted-process scenario (actually `kill -9` a live `ferric query` mid-run against a
  REAL backend, then `--resume` it) is a **manual verification step**, not automated — matches the
  project's established no-live-backend-CI position (ADR-045). The hand-written/real-truncated-
  trace fixtures above are the automated stand-ins.
