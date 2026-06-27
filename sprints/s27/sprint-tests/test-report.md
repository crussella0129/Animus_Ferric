# Sprint 27 Test Report — A no-progress guard for "semantic flailing" (ADR-031/037)

**Date:** 2026-06-27. Shipped the complement to the repetition guard: a same-tool-**name**
streak detector that catches the different-args flail the repetition guard misses, stopping
early with a precise `StopReason::NoProgress`. All tests green.

## Build / Lint (green)
- `cargo test --workspace` green (all crates); `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.

## Unit — `progress.rs` (`ferric-loop`) — 4/4 pass
- `warns_then_stops_on_a_same_name_streak`: same single-tool turn → `Proceed`×4, `Warn` (streak 4), `Stop` (streak 5).
- **`the_defining_contrast_same_tool_different_args`** — the proof: the same tool with **different args** each turn → `ProgressGuard` reaches `Stop` while `RepetitionGuard` returns `Proceed` on the *same* input. This is exactly the ADR-031 gap, asserted side by side.
- `a_tool_name_change_resets_the_streak`: a different tool mid-streak resets (no premature stop).
- `name_set_is_order_independent`: `{a,b}` and `{b,a}` (different args) are the same name set — the streak builds, reordering is not a reset.

## Integration — `progress_tests.rs` (`ferric-loop`, scripted provider) — 2/2 pass
- `flail_warns_then_stops_with_no_progress`: six `make_dir` turns with distinct paths → `outcome.stop == StopReason::NoProgress`; `NoProgressGuard` trace actions == `["warned","stopped"]`; `session_end` reason == `"no_progress"`; **the repetition guard stays silent** (every signature differs — the gap). The warn nudge (naming the tool + `task_complete`) is verified present in the request after the warn.
- `a_tool_change_resets_and_avoids_the_stop`: `make_dir` builds to a warn, then a different tool resets, then text → ends `FinalText`, exactly one `warned`, never a `stopped`.

## Regression
- Existing `repetition_tests` pass unchanged (identical-sig still stops at `RepetitionGuard`, which fires earlier than the looser progress streak).
- The two `loop_core` `max_turns` tests were updated to **alternate the tool name** across turns — they had filled the budget with a same-tool script, which the new guard (correctly) now stops first. Alternating names isolates the turn-budget stop again; both pass.

## Verdict
**No-progress guard validated.** The defining contrast test demonstrates the guard catches
exactly the mode ADR-031 flagged and the repetition guard misses. Honest scope holds: this
bounds wasted compute on a stuck model and emits a precise `no_progress` diagnostic (vs the
ambiguous `max_turns`) — it does not lift a model's capability ceiling. No bench change needed.
No human-verification checkpoint (logic fully covered by the deterministic scripted harness).
ADR-037.
