# Sprint 40 E2E Tests

- **Status:** possible (via `--mock`, no real GGUF model required) — and met by
  `real_run_compact_kill_replay_resume_shrinks_history` (`crates/ferric-loop/tests/
  compaction_tests.rs`), filed under `unit-tests.md`/`integration-tests.md`'s T-4004 section rather
  than duplicated here, matching sprints 38/39's precedent of not duplicating a test across files
  just to satisfy a section heading.
- That test is the strongest end-to-end proof in the suite: a REAL `run()` call scripted to trigger
  a real compaction (the summarizer call and the fold both actually execute), its REAL trace file is
  truncated to simulate a kill (dropping the trailing `session_end` line, mirroring sprint 39's
  C-010 technique exactly), `replay()`d, and the reconstructed history is asserted to be EXACTLY the
  expected shrunk size (head + 1 summary + the preserved tail — folded turns and the dangling final
  turn both correctly absent) — then a SECOND real `run()` resumes it and reaches `TaskComplete`.
  This is the concrete proof the sprint's motivating scenario (a compacted, then-interrupted session
  resuming without resurrecting its full pre-compaction history) actually works, not just in
  isolated units.
- A real interrupted-process scenario (`kill -9` a live `ferric query` mid-run against a REAL
  backend, after real compaction fired, then `--resume` it) remains a **manual verification step**,
  not automated — matches the project's established no-live-backend-CI position (ADR-045).
