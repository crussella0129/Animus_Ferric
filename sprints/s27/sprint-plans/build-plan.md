Finalized - DO NOT EDIT

# Sprint 27 Build Plan — A no-progress guard for "semantic flailing" (ADR-031)

Add the complement to the repetition guard: a same-tool-**name** streak detector that
catches ADR-031's second, still-unguarded failure mode (same tool, *different* args,
repeated → `max_turns`). Stop early with a precise `StopReason::NoProgress`. Honest scope:
bounds wasted compute + sharpens the bench diagnostic; does not lift a capability ceiling.
Rationale: `sprints/s27/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** a no-progress guard, wired + tested + recorded.
  - **A. the guard primitive + types** — T-2701
  - **B. loop integration** — T-2702
  - **C. ADR + docs** — T-2703

## Execution Sequence

### T-2701: `ProgressGuard` + `StopReason::NoProgress` + `Event::NoProgressGuard`
- **Touches:** `crates/ferric-loop/src/progress.rs` (new), `crates/ferric-loop/src/outcome.rs`, `crates/ferric-loop/src/lib.rs`, `crates/ferric-trace/src/event.rs`
- **Depends on:** —
- **Description:** `ProgressGuard{last_names, streak}` mirroring `RepetitionGuard`; signature = sorted-unique tool **names** (arg-insensitive); `observe(&[ToolCall]) -> Verdict{Proceed,Warn,Stop}` with `WARN_AT=4`/`STOP_AT=5`. Add `StopReason::NoProgress`→`"no_progress"` and `Event::NoProgressGuard{action}`; `pub mod progress`.
- **Success (EARS):**
  - WHEN the same tool-name set is observed for `STOP_AT` consecutive turns THEN `ProgressGuard` SHALL return `Stop`.
  - WHEN it is observed for exactly `WARN_AT` consecutive turns THEN `ProgressGuard` SHALL return `Warn`.
  - WHEN the tool-name set changes THEN `ProgressGuard` SHALL reset the streak and return `Proceed`.
  - WHEN converted THEN `StopReason::NoProgress.as_str()` SHALL equal `"no_progress"`.

### T-2702: Wire the guard into the loop
- **Touches:** `crates/ferric-loop/src/run.rs`
- **Depends on:** T-2701
- **Description:** construct `ProgressGuard` alongside the repetition guard (~L103); after the repetition-guard match (~L275) add a `progress.observe(&actions)` match — `Warn` → `Event::NoProgressGuard{"warned"}` + a course-correction nudge naming the repeated tool; `Stop` → `Event::NoProgressGuard{"stopped"}` + `break 'outer StopReason::NoProgress`.
- **Success (EARS):**
  - WHEN a model emits the same tool with different args past `STOP_AT` THEN the loop SHALL stop with `StopReason::NoProgress` and the trace SHALL end with reason `no_progress`.
  - WHEN the warn threshold is reached THEN a course-correction nudge SHALL be appended to the messages before the next turn.
  - WHEN actions repeat *identically* THEN the repetition guard SHALL still fire first (no regression).

### T-2703: ADR-037 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-2702
- **Description:** ADR-037 (closes ADR-031's second mode; names-only streak; WARN_AT/STOP_AT; composition with the repetition guard; honest scope + the documented false-positive tradeoff). README Status 27 + Sprint 27 timeline.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-037 and README SHALL show Sprint 27.

## Post-build (test)
- `cargo test -p ferric-loop` (new unit + integration) + `cargo test --workspace` green; clippy `-D warnings`; fmt.
