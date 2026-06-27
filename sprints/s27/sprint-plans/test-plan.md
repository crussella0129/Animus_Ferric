Finalized - DO NOT EDIT

# Sprint 27 Test Plan — A no-progress guard for "semantic flailing"

## Unit — `progress.rs` (`ferric-loop`, default CI)
- **warn→stop at threshold:** observe the same single-tool turn repeatedly → `Proceed`… then `Warn` at the `WARN_AT`-th consecutive match, `Stop` at the `STOP_AT`-th.
- **the defining contrast (ADR-031 gap):** the same tool with **different args** each turn → `ProgressGuard` reaches `Stop`, while `RepetitionGuard` returns `Proceed` on the *same* input (asserted side by side — this is the whole point).
- **reset on name change:** a streak interrupted by a turn using a *different* tool name resets (no `Stop`).
- **name-set granularity:** a turn calling two tools `{a,b}` repeated is matched by its sorted-unique name set; reordering the same names is still a match (no reset).

## Unit — `outcome.rs`
- `StopReason::NoProgress.as_str() == "no_progress"`.

## Integration — mirror `repetition_tests.rs` (`ferric-loop`, scripted provider)
- **flail → stop:** scripted same-tool/different-args turns past `STOP_AT` → `outcome.stop == StopReason::NoProgress`; a `NoProgressGuard`-filtered `guard_actions` == `["warned","stopped"]`; `session_end_reason == "no_progress"`. The warn nudge (naming the repeated tool / `task_complete`) is visible in the request after the warn.
- **name change avoids stop:** same tool for a few turns, then a different tool, then text → ends `StopReason::FinalText`, no `NoProgressGuard{"stopped"}`.

## Regression
- Existing `repetition_tests` pass unchanged: identical-signature turns still stop at `StopReason::RepetitionGuard` (it fires at 2 strikes, earlier than the looser progress streak).

## Build / Lint (default CI)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean. **No `ferric-bench`/`verify.rs` change** — `completed()` already passes only on `None|task_complete|final_text`, so a `no_progress` terminator classifies as a non-completion automatically.

## E2E
- Not required this sprint: the scripted-provider integration harness exercises the full loop deterministically, which is the right granularity for a guard. (A live bench would only re-confirm the 1B's known capability ceiling, not the guard's logic.)
