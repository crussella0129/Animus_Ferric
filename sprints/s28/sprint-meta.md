# Sprint 28 Meta

- **Sprint number:** 28
- **Start timestamp:** 2026-06-27T19:56:15Z
- **End timestamp:** 2026-06-27T20:25:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Added the **repeated-failure guard**, completing the loop-hardening guard family. The repetition guard (name+args) and no-progress guard (tool name) both key off the *actions* a model emits; neither catches a model emitting a *different* tool every turn that *all error* (both reset → grinds to `max_turns`). The new result-keyed `FailureGuard` (`crates/ferric-loop/src/failure.rs`) counts consecutive turns whose dispatched tools all errored — `observe_turn(dispatched, errored)`, Warn at WARN_AT=2, Stop at STOP_AT=3 → `StopReason::RepeatedFailure` (`Event::FailureGuard`), wired after the dispatch loop in `run.rs` (gated on a non-terminating turn). The three guards compose by threshold (repetition 2 < failure 3 < no-progress 5). 6 new tests incl. the integration that stops different-failing-tools while the other two stay silent. No bench change. ADR-038; README Status 28. Honest scope: bounds wasted compute + sharpens the diagnostic, does not lift a capability ceiling. One PR per sprint; `dev` clean (PR #13 merged).
