# Test Critique — Sprint 40

Reviewed by a foreground test-critic agent against `sprint-plans/build-plan.md` (locked),
`sprint-plans/test-plan.md`, and the three test-phase summaries, cross-checked against the actual
source (`compact.rs`, `run.rs`, `replay.rs`, `compaction_tests.rs`, `trace_cmd.rs`, `cli.rs`) and by
independently re-running the test suite and hand-tracing the arithmetic.

## C-001: `resume_seeds_compactor_numbering_consistently` uses a loose `>= 7` bound where the
arithmetic is fully determinate
- **Where:** `crates/ferric-loop/tests/compaction_tests.rs`, `through_turn >= 7` assertion.
- **Finding:** hand-tracing the fixture's arithmetic against the real constants
  (`KEEP_LAST_TURNS = 2`, resumed `turns: 7`) shows `through_turn` is deterministically EXACTLY 7,
  not merely "at least 7." A loose `>=` bound wouldn't catch an off-by-one regression in
  `fold_count`/`completed` indexing that shifted the result to 8, 9, or 10.
- **Response:** **tighten-assertion.** Changed to `assert_eq!(through_turn, 7, ...)`, matching the
  precision `maybe_compact_folds_older_turns_keeps_recent_tail` already applies elsewhere.

## C-002: `replay_applies_one_history_compaction`'s doc comment overstates what the event's
position proves for THAT specific test
- **Where:** `crates/ferric-loop/src/replay.rs`, `replay_applies_one_history_compaction`'s doc
  comment.
- **Finding:** tracing `replay()`'s real `HistoryCompacted` match arm shows it operates solely on
  `committed_turn_starts` (advanced only via `commit_and_reset!()` on the next `TurnStart`) — it
  never reads `pending`. The event would reconstruct identically if placed anywhere within turn 5's
  own span. What actually matters is that `HistoryCompacted` follows the `TurnStart` that commits
  the turns being folded — not its position relative to ITS OWN turn's `TurnEnd`. The doc comment's
  claim that "before its TurnEnd" is load-bearing for this test oversold the test's own
  discriminating power (the test itself is correct and stays valuable as a realistic-shape
  fixture).
- **Response:** **tighten (comment only, not the test).** Corrected the doc comment to state the
  positioning mirrors real `run.rs` output for fixture realism, not that this specific test would
  fail if the event moved elsewhere within the turn's span — the actual byte-order invariant is
  verified end-to-end by `history_compacted_traced_after_triggering_turn_start` in
  `compaction_tests.rs`, which the corrected comment now cross-references.

## C-003: no test pins the exact 85%-of-budget boundary value itself
- **Where:** `crates/ferric-loop/src/compact.rs`'s trigger comparison (`tokens < 0.85 *
  prompt_budget_tokens`).
- **Finding:** all existing tests use values comfortably clear of the 2380-token boundary (100 for
  below-trigger, 2500 for above) — the `<` vs `<=` boundary semantics at exactly 2380 were
  unverified.
- **Response:** **add-test** (minor, as suggested — cheap and directly strengthens correctness
  confidence). Added `maybe_compact_trigger_boundary_is_exclusive_below` to `compact.rs`'s test
  module: `tokens == 2380` triggers, `tokens == 2379` does not.

## C-004: benign observation, no action needed
- **Finding:** `resume_only_folds_new_post_resume_turns` also incidentally validates `head_len`
  correctness beyond its stated description.
- **Response:** **reject** — confirmed non-issue; accurately described, not worth a test-plan edit.

## Verified as sound (critic's independent checks, no concern)
- All T-4001–T-4005 EARS clauses have corresponding tests; every claimed test name exists, passes,
  and counts match exactly (ferric-trace 12, ferric-loop lib 42, compaction_tests.rs 5, ferric-cli
  28).
- `maybe_compact_folds_older_turns_keeps_recent_tail` genuinely proves the in-flight-turn exclusion
  (plan-critic C-002) — traced both the correct and a hypothetical buggy implementation; the buggy
  version would produce a different `(through_turn, dropped_turns)` tuple the test's assertions
  would catch.
- `real_run_compact_kill_replay_resume_shrinks_history`'s "exactly 7" claim is arithmetically
  correct, independently re-derived by the critic against the real constants.
- The script-ordering fix (summarizer completion scripted before the triggering turn's own next
  completion) correctly reflects real `run.rs` call order — not implementation-coupling masking a
  bug.
- No EARS-coverage gaps, no stub-leakage, no integration-scope drift, no e2e cop-out, no flake risk.

## Confidence
proceed-with-caveats → all caveats addressed inline above; re-verified green after fixes.
