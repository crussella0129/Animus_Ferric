# Sprint 40 Meta

- **Sprint number:** 40
- **Start timestamp:** 2026-07-04T22:25:10Z
- **End timestamp:** 2026-07-05T00:10:00Z
- **Model:** claude-sonnet-5
- **Exit status:** success
- **Token count:** (not observable in this session)
- **Note:** Two build-phase task pairs shipped as single commits under Rust's compiler-enforced
  coupling, disclosed in their commit messages and `completed-tasks.md` (T-4001+T-4005: a new
  `Event` variant forces every exhaustive match site to update together; T-4002+T-4003:
  `HistoryCompactor` is `pub(crate)`-only, tripping dead-code analysis under `-D warnings` until
  `run.rs` calls it) — not a process deviation, a real constraint of this project's own lint gate.
- **Summary:** Context-budget compaction — enforce `RunPolicy.prompt_budget_tokens` via an always-on
  `HistoryCompactor` that folds older turns into one model-summarized message when `input_tokens`
  crosses 85% of budget, plus the required `replay()` extension so a resumed session doesn't
  resurrect pre-compaction history. Carved out of sprint 39's research, user-confirmed 2026-07-04.
