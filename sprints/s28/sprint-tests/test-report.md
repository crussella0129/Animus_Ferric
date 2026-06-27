# Sprint 28 Test Report — A repeated-failure guard (ADR-038)

**Date:** 2026-06-27. Shipped the third loop-hardening guard — the only one keying off tool
*results*. It catches the "different tools, all failing" mode the repetition + no-progress
guards reset on, stopping early with a precise `StopReason::RepeatedFailure`. All tests green.

## Build / Lint (green)
- `cargo test --workspace` green (all crates); `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.

## Unit — `failure.rs` (`ferric-loop`) — 4/4 pass
- `warns_then_stops_on_an_all_failed_streak`: `observe_turn(1,1)` repeated → `Proceed` (streak 1), `Warn` (2 == WARN_AT), `Stop` (3 == STOP_AT).
- `a_successful_call_resets_the_streak`: a turn with a success (`observe_turn(2,1)`) resets; the streak restarts from zero.
- `a_zero_dispatch_turn_never_trips`: `observe_turn(0,0)` ×5 → always `Proceed` (no result-bearing tool).
- `multi_call_turns_count_when_all_error`: `(1,1)`,`(3,3)`[Warn],`(2,2)`[Stop] — all-error counts regardless of calls/turn.

## Integration — `failure_tests.rs` (`ferric-loop`, scripted provider, real dispatch) — 2/2 pass
- **`all_failing_turns_warn_then_stop_with_repeated_failure`** — the proof: three turns with **different** tools on missing paths (`read_file`→`list_dir`→`read_file`, all erroring) → `outcome.stop == StopReason::RepeatedFailure`; `FailureGuard` trace actions == `["warned","stopped"]`; `session_end == "repeated_failure"`; **the repetition + no-progress guards stay silent** (different tools/args is exactly their gap). The warn nudge (mentioning `task_complete`) is verified present in the request after the 2nd failure.
- `a_success_resets_and_avoids_the_stop`: two failing turns (a warn), then a **succeeding** `list_dir "."`, then text → ends `FinalText`, exactly one `warned`, no `stopped`.

## Regression
- `repetition_tests`, `progress_tests`, `loop_core` pass unchanged. The failure integration deliberately varies tool names + args so the **failure** guard is what fires (a same-args repeated failing call would trip the repetition guard first at 2 identical strikes — the guards compose, earlier-threshold wins).

## Verdict
**Repeated-failure guard validated.** The integration test demonstrates the gap concretely:
a model emitting different failing tools every turn — which both action-keyed guards reset
on — is stopped by the result-keyed failure guard at 3 all-error turns. The loop-hardening
guard family (repetition / no-progress / repeated-failure) is complete and composes by
threshold (2 < 3 < 5). Honest scope holds: it bounds wasted compute and sharpens the
diagnostic, not a model's capability ceiling. No bench change; no human checkpoint (logic
fully covered by the deterministic scripted harness with real tool dispatch). ADR-038.
