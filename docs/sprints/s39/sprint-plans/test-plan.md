Finalized - DO NOT EDIT

# Sprint 39 Test Plan

## Unit Tests

### T-3901/T-3902 — trace format extensions
- `event::tests::session_prompt_roundtrip` (or folded into `ferric-trace`'s existing
  `all_event_types`/`jsonl_roundtrip_all_event_types` list): `Event::SessionPrompt` round-trips.
- `event::tests::session_start_resumed_from_roundtrip`: `SessionStart{workspace, resumed_from:
  Some(...)}` round-trips.
- `event::tests::old_session_start_line_parses_with_none_resumed_from`: a pre-sprint-39
  `session_start` line (no `resumed_from` key) still parses as `Known` with `resumed_from: None` —
  extends the existing `s0_trace_still_parses`-style fixture test.
- `event::tests::old_turn_end_line_parses_with_truncated_false`: a pre-sprint-39 `turn_end` line (no
  `truncated` key) still parses as `Known` with `truncated: false`.
- `event::tests::turn_end_truncated_roundtrip`: `TurnEnd{..., truncated: true}` round-trips.
- `ferric-loop` integration: a `NativeTools` mock run whose script terminates via `task_complete`
  produces a `ToolCall` trace event for the terminator (name/args match), and no corresponding
  `ToolResult`/registry-execute side effect (proves it's traced but not dispatched).

## Integration Tests

### T-3903 — `replay`
Synthetic trace files built via a real `JsonlSink` (matching the project's existing test idiom, no
new fixture-generation dependency):
- `replay_reconstructs_a_clean_constrained_json_session`: `SessionPrompt` + `PolicySelected` +
  2 turns (tool call + result each), no `SessionEnd` → `messages` matches exactly what `run()`
  would hold at that point (system, user, assistant(turn0 raw json), tool-result(turn0), assistant
  (turn1 raw json), tool-result(turn1)); `turns == 2`; `protocol == ConstrainedJson`.
- `replay_preserves_native_multi_tool_call_order`: a `NativeTools` turn with 2+ `ToolCall` events
  between one `TurnStart`/`TurnEnd` pair → the reconstructed assistant message's `tool_calls` is in
  the same order.
- **(C-003, plan-critic)** `replay_preserves_terminator_position_mid_turn`: a `NativeTools` turn
  whose ORIGINAL action order was `[tool_a, task_complete, tool_b]` (terminator in the MIDDLE, not
  last) — the reconstructed assistant message's `tool_calls` preserves that exact order, not
  `[tool_a, tool_b, task_complete]`. Directly targets the ordering ambiguity the critic flagged;
  would fail if the terminator's trace-write were placed after the dispatch loop instead of inline.
- `replay_reconstructs_repetition_guard_nudge`: a turn followed by `RepetitionGuard{action:
  "warned"}` → the reconstructed nudge message names the same tool(s) that turn's `ToolCall` events
  named, using the repetition-specific formatter (byte-identical to what `run()` would have pushed —
  NOT the no-progress-guard's different wording).
- `replay_reconstructs_no_progress_guard_nudge`: same shape, `NoProgressGuard{action:"warned"}` →
  the no-progress-specific wording (distinct from repetition's) — proves the two guard formatters
  aren't collapsed into one generic template (C-007).
- **(C-005, plan-critic)** `replay_reconstructs_xml_parse_error_nudge_falls_back_to_generic`: a
  `TextXml` turn with zero `ToolCall` events (a parse failure occurred in the original run, its exact
  error text unrecoverable) → `replay` produces the generic protocol-keyed no-action nudge, not a
  panic or an empty message.
- `replay_reconstructs_truncated_turn_retry`: a `TurnEnd{truncated: true}` → the truncation-retry
  text is appended, and that turn's (partial) text is NOT treated as an action.
- **(C-001, plan-critic)** `replay_discards_a_dangling_mid_turn`: a trace whose LAST event is a
  `TurnStart` with NO matching `TurnEnd` (the realistic shape of a killed process — most interrupted
  sessions die mid-turn, not exactly on a turn boundary) → `replay` succeeds, `turns` counts only the
  earlier COMPLETED turns, and no partial/assistant message is added for the dangling turn.
- `replay_missing_session_prompt_is_an_error`: a trace with no `SessionPrompt` →
  `ReplayError::MissingSessionPrompt`.
- `replay_already_stopped_is_an_error`: a trace with a `SessionEnd` → `ReplayError::AlreadyStopped`
  carrying the original reason.

### T-3904 — `RunArgs.resume`/`run()`
- **Regression:** every pre-existing `ferric-loop` test file (`loop_core`, `terminator_tests`,
  `repetition_tests`, `progress_tests`, `failure_tests`, `backoff_tests`, `grammar_loop`,
  `constrained_loop`, `truncation_tests`, `streaming_tests`) continues to pass with `resume: None`
  and `prompt` wrapped in `Some(...)` at each call site — proves byte-identical behavior for the
  `None` case.
- `resume_some_continues_from_replayed_state`: `run()` driven with `resume: Some(replayed)` and
  `prompt: None` against a scripted `MockProvider` — the resulting `LoopOutcome.turns` continues
  from `replayed.turns` (not reset to 0), and the trace shows `SessionStart.resumed_from ==
  Some(replayed.source_session)` with no new `SessionPrompt` event.
- `resume_some_with_extra_prompt_appends_one_user_message`: `resume: Some(...)`, `prompt:
  Some("extra")` → the extra message is appended after the replayed history, before the next
  provider call.
- `resume_none_prompt_none_is_an_error_not_a_panic`: `run()` called with `resume: None`, `prompt:
  None` → returns `Err(...)`, does not panic.
- **(C-010, plan-critic — the single strongest regression test)** `real_run_then_replay_then_resume_
  reaches_task_complete`: a genuine round-trip, not another hand-built fixture. (1) Drive a REAL
  `run()` call against a scripted `MockProvider` for a multi-turn task; (2) truncate the resulting
  REAL trace file by dropping its trailing `SessionEnd` line (simulating a kill after the last
  completed turn); (3) call `ferric_loop::replay()` on that truncated REAL trace; (4) drive a SECOND
  `run()` call with `resume: Some(replayed)` against a continuation `MockProvider` script that
  finishes the task; (5) assert the full round-trip reaches `StopReason::TaskComplete`. This is the
  test most likely to catch drift between what `run()` actually emits and what `replay()` assumes —
  every other replay test builds its trace fixture by hand.

### T-3905 — `ferric query --resume <path>` CLI
- `cli::resume_continues_an_interrupted_session`: a hand-written partial trace fixture (matching
  T-3903's synthetic-trace approach: `SessionPrompt` + `PolicySelected` + one completed turn, no
  `SessionEnd`) written to a tempdir workspace's `.ferric/trace/`; run `ferric query --mock
  --resume <path>` (no prompt) against the REAL binary; assert success and that the NEW trace shows
  `resumed_from` pointing at the original session id.
- `cli::resume_with_extra_prompt_appends_nudge`: same fixture, `--resume <path> "extra instruction"`
  → succeeds, the new trace's `prompt_assembled` char count reflects the appended message.
- `cli::no_resume_and_no_prompt_is_a_usage_error`: `ferric query --mock` with neither `--resume` nor
  a prompt → fails with a usage error (regression proving today's "prompt required" behavior is
  unchanged when `--resume` is never used).
- `cli::resume_protocol_mismatch_is_a_clear_error`: a fixture trace recorded under `ConstrainedJson`,
  resumed with flags that resolve to `NativeTools` → fails with an error naming both protocols, no
  run attempted.
- `cli::resume_already_stopped_is_a_clear_error`: a fixture trace that already has a `SessionEnd` →
  fails with an error naming the original stop reason, no run attempted.
- **(C-009, plan-critic)** `cli::resume_with_animus_md_prints_ignored_note`: the same interrupted-
  session fixture, an `Animus.md` present at the workspace root, `--resume <path>` (no
  `--prompts-dir`) → stderr contains a note that `Animus.md` is ignored for this resumed run's
  (frozen) system message.

## End-to-End Tests
- **Status:** possible (via `--mock`, no real GGUF model required) — and largely covered by
  T-3905's CLI subprocess tests above, which already exercise a real `ferric` binary against a real
  trace file on real disk (the project's established "subprocess + real disk" e2e bar, per sprint
  38's precedent).
- A real interrupted-process scenario (actually `kill -9` a live `ferric query` mid-run against a
  real backend, then `--resume` it) is a **manual verification step**, not automated — matches the
  project's established no-live-backend-CI position (ADR-045). The hand-written partial-trace
  fixture is the automated stand-in, since a `--mock` run reaching `MockProvider::ScriptExhausted`
  or being killed mid-process isn't a clean way to produce a genuinely-truncated trace file inside a
  test process.

## Build / Lint (all tasks)
- `cargo test --workspace` green.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all --check` clean.
