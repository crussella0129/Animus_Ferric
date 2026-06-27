Finalized - DO NOT EDIT

# Sprint 28 Test Plan — A repeated-failure guard

## Unit — `failure.rs` (`ferric-loop`, default CI)
- **warn→stop on an all-failed streak:** `observe_turn(1, 1)` repeated → `Proceed` (streak 1), `Warn` (streak 2 == WARN_AT), `Stop` (streak 3 == STOP_AT).
- **a success resets:** a turn with `observe_turn(2, 1)` (one of two calls succeeded) → `Proceed` and the streak resets; a subsequent all-failed turn starts from 0.
- **zero-dispatch turn:** `observe_turn(0, 0)` → `Proceed`, never trips (no result-bearing tool).
- **partial then full:** `(1,1)`,`(1,1)` [Warn], `(2,2)` [Stop] — all-error counts regardless of call count per turn.

## Unit — `outcome.rs`
- `StopReason::RepeatedFailure.as_str() == "repeated_failure"`.

## Integration — `run_scripted` (`ferric-loop`, scripted provider)
- **fail → stop:** a tool that errors every turn with **different** args (so repetition + no-progress both reset — e.g. `read_file` on missing paths `missing0…2`, or `delete_path` into a denied scope) for 3 turns → `outcome.stop == StopReason::RepeatedFailure`; `FailureGuard`-filtered actions == `["warned","stopped"]`; `session_end == "repeated_failure"`; the warn nudge is present in the request after the 2nd failure.
- **a success avoids the stop:** two failing turns, then a **succeeding** tool call, then text → ends `StopReason::FinalText`, exactly one `warned`, no `stopped`.

## Regression
- `repetition_tests`, `progress_tests`, `loop_core` unaffected. The failure integration deliberately varies args so the **failure** guard fires (a same-args repeated failing call would trip the repetition guard first at 2 identical strikes — the guards compose, earlier-threshold wins).

## Build / Lint (default CI)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean. The two exhaustive `Event` matches (`tests/common/mod.rs::kinds`, `crates/ferric-cli/src/trace_cmd.rs`) get a `FailureGuard` arm (compiler-enforced). **No `verify.rs` change** — `completed()` already treats a non-`task_complete`/`final_text` terminator as a non-completion.

## E2E
- Not required this sprint: the scripted-provider integration harness exercises the full loop with real tool dispatch (errors included) deterministically — the right granularity for a result-keyed guard.
