Finalized - DO NOT EDIT

# Sprint 28 Build Plan — A repeated-failure guard (the third loop-hardening guard)

Add the result-keyed guard the loop is missing: track consecutive turns whose dispatched
tools **all errored**, and stop early with `StopReason::RepeatedFailure`. Catches the
"different tools, all failing" mode that repetition (resets on signature) and no-progress
(resets on name) both miss. Honest scope: bounds wasted compute + sharpens the diagnostic;
does not lift a capability ceiling. Rationale: `sprints/s28/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** a repeated-failure guard, wired + tested + recorded.
  - **A. the guard primitive + types** — T-2801
  - **B. dispatch-loop integration** — T-2802
  - **C. ADR + docs** — T-2803

## Execution Sequence

### T-2801: `FailureGuard` + `StopReason::RepeatedFailure` + `Event::FailureGuard`
- **Touches:** `crates/ferric-loop/src/failure.rs` (new), `crates/ferric-loop/src/outcome.rs`, `crates/ferric-loop/src/lib.rs`, `crates/ferric-trace/src/event.rs`
- **Depends on:** —
- **Description:** `FailureGuard{consecutive_failed_turns}`; `observe_turn(dispatched, errored) -> Verdict` (reuse `crate::repetition::Verdict`); `all_failed = dispatched > 0 && errored == dispatched`; reset on `!all_failed`; `WARN_AT=2`/`STOP_AT=3`. Add `StopReason::RepeatedFailure`→`"repeated_failure"`, `Event::FailureGuard{action}`, `mod failure`.
- **Success (EARS):**
  - WHEN a turn dispatches ≥1 tool and every dispatched tool errors THEN `observe_turn` SHALL increment the streak.
  - WHEN the streak reaches `STOP_AT` THEN it SHALL return `Stop`; at `WARN_AT`, `Warn`.
  - WHEN any dispatched tool in a turn succeeds THEN it SHALL reset the streak and return `Proceed`.
  - WHEN converted THEN `StopReason::RepeatedFailure.as_str()` SHALL equal `"repeated_failure"`.

### T-2802: Wire into the dispatch loop
- **Touches:** `crates/ferric-loop/src/run.rs`
- **Depends on:** T-2801
- **Description:** construct `FailureGuard` with the other guards; in the dispatch `for` loop count dispatched non-terminator calls + `is_error` count; after the loop, if `terminate_with.is_none()` and ≥1 dispatched, `failure.observe_turn(..)` — `Warn` → `Event::FailureGuard{"warned"}` + a nudge; `Stop` → `Event::FailureGuard{"stopped"}` + `break 'outer StopReason::RepeatedFailure`.
- **Success (EARS):**
  - WHEN `STOP_AT` consecutive all-error turns occur THEN the loop SHALL stop with `StopReason::RepeatedFailure` and the trace SHALL end `repeated_failure`.
  - WHEN a turn calls `task_complete` THEN the failure guard SHALL NOT fire for that turn.
  - WHEN the warn threshold is reached THEN a course-correction nudge SHALL be appended before the next turn.

### T-2803: ADR-038 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-2802
- **Description:** ADR-038 (completes the guard family; result-keyed post-dispatch all-errored streak; WARN/STOP=2/3; honest scope + false-positive tradeoff). README Status 28 + Sprint 28 timeline.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-038 and README SHALL show Sprint 28.

## Post-build (test)
- `cargo test -p ferric-loop` (new unit + integration) + `cargo test --workspace` green; clippy `-D warnings`; fmt. Two exhaustive `Event` matches get a `FailureGuard` arm.
