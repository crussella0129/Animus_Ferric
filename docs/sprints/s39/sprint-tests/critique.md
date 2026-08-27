# Test Critique — Sprint 39

Reviewed by a foreground test-critic agent against `sprint-plans/build-plan.md` (locked),
`sprint-plans/test-plan.md`, and the three test-phase summaries, cross-checked against the actual
source (`replay.rs`, `run.rs`, `query.rs`, `resume_tests.rs`, `cli.rs`) and by independently
re-running the test suite.

## C-001: terminator mid-turn ordering (T-3902/C-003) tested only on replay's read side
- **Where:** `replay_preserves_terminator_position_mid_turn` (`crates/ferric-loop/src/replay.rs`)
  hand-builds its fixture via direct `JsonlSink::write_event` calls in the already-correct order —
  it never drives a real `run()` dispatch loop with a scripted `[tool_a, task_complete, tool_b]`
  turn. `replay()` itself has no terminator-aware logic (it just preserves file order), so this test
  can't distinguish a correct inline trace-write in `run.rs` from a regression that moved it after
  the dispatch loop. `terminator_tests::task_complete_mixed_turn` (the one test that DOES drive a
  real multi-call turn) uses `[write_file, task_complete]` (terminator last) and asserts nothing
  about trace order.
- **Response:** **add-test.** Extended `terminator_tests.rs` with a new test that scripts
  `[tool_a, task_complete, tool_b]` through a real `run()` call and asserts the resulting trace's
  `tool_call` name order matches original emission order exactly.

## C-002: `resume_with_extra_prompt_appends_nudge` (CLI) — vacuous char-count floor
- **Where:** `crates/ferric-cli/tests/cli.rs`, asserted `max_chars >= "extra instruction".len()`.
  Verified empirically: without the extra prompt at all, `max_chars` is already 74 (base replayed
  history) — comfortably clearing the 17-char bar regardless of whether the extra prompt is ever
  appended. Doesn't prove T-3905's second EARS clause (extra prompt appended as one extra message).
- **Response:** **tighten-assertion.** Replaced the loose floor with an exact-delta assertion:
  compare `max_chars` against a same-fixture run WITHOUT the extra prompt, asserting the difference
  equals exactly `"extra instruction".len()` (mirrors `resume_some_with_extra_prompt_appends_one_
  user_message`'s direct in-process check, adapted to the CLI's char-count-only trace surface).

## C-003: no negative test that the ignored-note is resume-gated
- **Where:** `animus_md_present_traces_note` (no `--resume`) never asserts the "ignores
  --prompts-dir/Animus.md" note is ABSENT from stderr. A regression making the `query.rs` guard
  unconditional would go undetected.
- **Response:** **add-test.** Extended `animus_md_present_traces_note` with an assertion that stderr
  does NOT contain the ignored-note string when `--resume` is absent.

## C-004: build-plan.md's EARS wording for the dangling-turn clause is narrower than shipped/tested
- **Where:** locked EARS says "no matching TurnEnd"; shipped behavior is the stricter "no confirming
  next TurnStart" (both cases are in fact tested: `replay_discards_a_dangling_mid_turn` and
  `replay_discards_a_turn_end_with_no_confirming_next_turn_start`), and ADR-049 documents the
  build-time discovery honestly as a strict superset.
- **Response:** **reject** — confirmed non-issue. Both cases are tested; the stricter behavior is
  disclosed in ADR-049. build-plan.md is intentionally frozen ("DO NOT EDIT") per the sprint-loop
  schema, so its literal wording predating the build-time discovery is expected and not amended.

## C-005: no replay test combines non-empty `text` + non-empty `tool_calls` for `NativeTools`
- **Where:** `PendingTurn::finalize`'s `NativeTools` branch copies both fields verbatim, but every
  existing replay test exercises them in isolation (text-only or tool_calls-only), never together.
- **Response:** **add-test** (low priority). Extended `replay_preserves_native_multi_tool_call_order`
  with a variant carrying `TurnEnd.text = Some("thinking...")` alongside 2 `ToolCall` events,
  asserting both land in the reconstructed assistant message.

## Verified as sound (critic's independent checks, no concern)
- Terminator `ToolCall` placement in `run.rs` is correctly inline at the original `continue`
  (implementation is right; C-001's gap was in test coverage only).
- `write_interrupted_trace_fixture`'s hand-written JSON matches real serde wire format exactly
  (all omitted fields are legitimately `#[serde(default...)]`-omittable).
- All numeric test-count claims and `clippy`/`fmt` claims independently reproduced and match.
- `real_run_then_replay_then_resume_reaches_task_complete` (C-010) is genuinely the strongest test
  in the suite; the e2e-tests.md non-duplication claim is honest, not a cop-out.
- `resume: None`/`resume: Some` regression mechanics confirmed mechanical-only via `git show`.
- `resume_some_continues_from_replayed_state`'s `turns == 6` assertion is non-vacuous (a
  reset-to-0 bug would produce 1, not 6).
- Guard-nudge formatter distinctness and the `TextXml` fallback both correctly exercised.
- ADR-049 confirmed to cover everything T-3906's EARS clause requires.

## Confidence
proceed-with-caveats → all caveats addressed inline below; re-verified green after fixes.
