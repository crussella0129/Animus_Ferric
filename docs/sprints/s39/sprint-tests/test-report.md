# Sprint 39 Test Report — session resume (`ferric query --resume <path>`)

## Summary
All 6 build tasks (T-3901–T-3906) are covered by tests derived directly from `build-plan.md`'s
locked EARS clauses. A foreground test-critic agent reviewed the plan/test-plan against the written
test summaries and the actual source, surfacing 5 concerns (2 concrete/actionable, 1 minor
negative-path gap, 1 minor untested field combination, 1 confirmed non-issue). All actionable
concerns were fixed inline; the suite was re-verified green after each fix. See `critique.md` for
the full critique text and per-concern responses.

## Critic findings and resolutions
- **C-001** (add-test): the plan-critic's most significant finding (terminator mid-turn trace-write
  ordering, T-3902) was previously verified only via a hand-built `replay()` fixture that couldn't
  distinguish a correct `run.rs` implementation from a regression. Fixed by adding
  `terminator_tests::task_complete_mid_turn_preserves_emission_order`, which drives a REAL `run()`
  call with `[write_file, task_complete, write_file]` in one turn and asserts the traced order.
- **C-002** (tighten-assertion): `resume_with_extra_prompt_appends_nudge`'s char-count floor was
  empirically vacuous (passed with or without the extra prompt). Fixed by comparing against a
  same-fixture baseline run with no extra prompt and asserting an exact delta.
- **C-003** (add-test, minor): no test proved the `--resume`+`Animus.md` ignored-note stays silent
  on ordinary runs. Fixed by extending `animus_md_present_traces_note` with a negative assertion.
- **C-004** (reject): build-plan.md's literal EARS wording for the dangling-turn clause predates the
  build-time discovery of the stricter rule; both the literal and stricter cases are in fact tested,
  and ADR-049 discloses the refinement. No action needed.
- **C-005** (add-test, minor): no replay test combined non-empty `text` with non-empty `tool_calls`
  for `NativeTools`. Fixed by adding `replay_preserves_native_text_alongside_tool_calls`.

## Coverage by task
- **T-3901/T-3902** (trace format extensions): round-trip + backward-compat tests in
  `ferric-trace`; `ferric-loop` regression tests updated to assert the terminator IS traced but
  NOT dispatched; new C-001 test proves the trace-write position itself, not just replay's
  order-preservation.
- **T-3903** (`ferric-loop::replay`): 13 co-located unit tests (12 original + C-005's addition)
  covering clean reconstruction, tool-call ordering (incl. terminator mid-turn), both guard nudges
  (with distinctness proof), the `TextXml` fallback, truncation retry, both discard cases (dangling
  and TurnEnd-without-confirming-TurnStart), and both error paths.
- **T-3904** (`RunArgs.resume`/`run()`): full pre-existing regression suite unchanged in behavior +
  4 new tests incl. the real round-trip (`real_run_then_replay_then_resume_reaches_task_complete`).
- **T-3905** (CLI wiring): 6 subprocess tests against a hand-verified wire-format-accurate fixture,
  covering both success paths, both error paths, the usage-error regression, and the C-009 note
  (now with its C-003 negative-path sibling).
- **T-3906** (ADR-049 + docs): no test surface — verified by direct read during the critic pass.

## Final verification
- `cargo test --workspace`: all green (`ferric-trace` 11, `ferric-loop` lib 34, `ferric-loop`
  `terminator_tests` 6, `ferric-cli` `cli` 27, plus all other unaffected suites).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- `cargo build --workspace --features backend-openai` / `--features backend-mistralrs`: both clean.

## Confidence
Clean — proceed to Loop Phase.
