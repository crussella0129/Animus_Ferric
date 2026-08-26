# Sprint 39 Unit Tests

All derived from the locked `build-plan.md` EARS clauses (one test per WHEN/THEN/SHALL triple),
incl. the plan-critique's C-001/C-003/C-004/C-005/C-006/C-007/C-010 fixes folded in during the
build. All green.

## T-3901/T-3902 — trace format extensions
- `event::tests`/`lib.rs` tests (folded into `ferric-trace`'s existing round-trip/backward-compat
  idiom): `session_prompt_roundtrip_with_media`, `session_start_resumed_from_roundtrip`,
  `old_session_start_line_parses_with_none_resumed_from` (a pre-sprint-39 `session_start` line, no
  `resumed_from` key, still parses `Known` with `None`), `turn_end_truncated_roundtrip`,
  `old_turn_end_line_parses_with_truncated_false` (same backward-compat proof for `TurnEnd`).
- `all_event_types()`/`jsonl_roundtrip_all_event_types` extended with a `SessionPrompt` entry and
  the new fields on `SessionStart`/`TurnEnd` — proves the full vocabulary (old + new) still
  round-trips as one set.
- `ferric-loop` regression: `terminator_tests::task_complete_terminates` and
  `grammar_loop::textxml_terminator_intercepted` — previously asserted "no `tool_call` event for the
  terminator" (actually testing non-dispatch); corrected to assert the real invariant (`tool_call`
  now present per T-3902, `tool_result` still absent — traced but never dispatched).
- `terminator_tests::task_complete_mid_turn_preserves_emission_order` (test-critic C-001) — a real
  `run()` call scripted with `[write_file, task_complete, write_file]` in ONE turn, asserting the
  traced `tool_call` name order matches emission order exactly and only the 2 non-terminator calls
  dispatch. Closes the gap where `replay_preserves_terminator_position_mid_turn` (below) only proved
  replay doesn't reorder an already-correct fixture — it never verified `run.rs`'s dispatch loop
  itself writes the terminator's trace event at the correct (inline) position.

## T-3903 — `ferric-loop::replay` (co-located `#[cfg(test)]` in `replay.rs`, not `tests/`)
Co-located rather than in `tests/` because the extracted nudge-formatting helpers are `pub(crate)`
and invisible to the external `tests/` crate boundary.
- `replay_reconstructs_a_clean_constrained_json_session` — a 2-turn `ConstrainedJson` session
  reconstructs `messages` exactly; `turns == 2`; `protocol == ConstrainedJson`.
- `replay_preserves_native_multi_tool_call_order` — a `NativeTools` turn with 2 `ToolCall` events
  preserves order in the reconstructed assistant message.
- `replay_preserves_terminator_position_mid_turn` (C-003) — the terminator MID-turn (not last)
  preserves exact original order — the direct regression test for the critic's most significant
  finding (the terminator trace-write's loop position).
- `replay_reconstructs_repetition_guard_nudge` / `replay_reconstructs_no_progress_guard_nudge`
  (C-007) — both guard nudges reconstruct with their OWN distinct wording, proving they aren't
  collapsed into one generic formatter.
- `replay_reconstructs_xml_parse_error_nudge_falls_back_to_generic` (C-005) — the accepted
  `TextXml` parse-error approximation produces a valid, non-empty, non-panicking nudge.
- `replay_reconstructs_truncated_turn_retry` — the truncation retry text, no assistant message for
  the truncated turn.
- `replay_discards_a_dangling_mid_turn` (C-001) — a `TurnStart` with no matching `TurnEnd` at all
  (the realistic killed-process shape) is discarded, not counted.
- `replay_discards_a_turn_end_with_no_confirming_next_turn_start` — the STRICTER refinement found
  during implementation (C-001's build-time correction): even a turn WITH a `TurnEnd` is discarded
  if no later `TurnStart` confirms its dispatch finished.
- `replay_missing_session_prompt_is_an_error` / `replay_already_stopped_is_an_error` — both error
  paths.
- `replay_preserves_native_text_alongside_tool_calls` (test-critic C-005) — a `NativeTools`
  completion carrying BOTH prose text and tool calls in the same message reconstructs both fields
  (every other test exercised them in isolation).

## T-3904 — `RunArgs.resume`/`run()` (`crates/ferric-loop/tests/resume_tests.rs`)
- **Regression:** every pre-existing `ferric-loop` test file, unmodified except threading
  `Some(prompt)`/`resume: None` at each call site, keeps passing — the primary proof of
  byte-identical behavior for the `resume: None` case.
- `resume_some_continues_from_replayed_state` — `turns` continues from the replayed count (not
  reset to 0); `resumed_from` traced; no new `SessionPrompt` written.
- `resume_some_with_extra_prompt_appends_one_user_message` — the extra nudge reaches the provider's
  first request.
- `resume_none_prompt_none_is_an_error_not_a_panic` — returns `Err`, doesn't panic.
- `real_run_then_replay_then_resume_reaches_task_complete` (C-010, the strongest regression) — a
  REAL `run()` call reaches `TaskComplete`; its REAL trace file is truncated (drop the trailing
  `SessionEnd` line); `replay()`d; a SECOND real `run()` resumes it and reaches `TaskComplete`
  again. The only test in the suite that round-trips through the actual `run()`/`replay()` boundary
  rather than a hand-built fixture on either side.

## Result
`cargo test -p ferric-trace`: 11 passed (up from 6). `cargo test -p ferric-loop` (lib): 34 passed
(up from 21, incl. C-005's fix). `cargo test -p ferric-loop --test terminator_tests`: 6 passed (up
from 5, incl. C-001's fix). `cargo test -p ferric-loop` (integration, all files incl.
`resume_tests.rs`): all green. `--features backend-openai`/`backend-mistralrs`: unaffected, both
clean. See `critique.md` for the test-critic pass and fixes (C-001 through C-005).
