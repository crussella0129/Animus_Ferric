# Sprint 40 Unit Tests

All derived from the locked `build-plan.md` EARS clauses (one test per WHEN/THEN/SHALL triple),
incl. the plan-critique's C-001/C-002/C-003/C-004/C-005/C-006/C-007 fixes folded in during the
build. All green.

## T-4001 — `Event::HistoryCompacted` new trace variant
- `history_compacted_roundtrip` (`ferric-trace/src/lib.rs`): `through_turn`/`dropped_turns`/
  `summary` survive serialize→deserialize exactly.
- `all_event_types()`/`jsonl_roundtrip_all_event_types` extended with one `HistoryCompacted` entry
  — full vocabulary (old + new) still round-trips as one set.
- No backward-compat fixture test needed (unlike sprint 39's `SessionStart.resumed_from`/
  `TurnEnd.truncated`): this is a brand-new variant, not an extension of an existing one — old
  readers already tolerate unknown variants per ADR-002, with no new field defaulting to prove.

## T-4002 — `HistoryCompactor` + pure helpers (`crates/ferric-loop/src/compact.rs`, co-located
`#[cfg(test)]`)
- `render_transcript_names_roles_and_tool_calls` — a slice of system/user/assistant/tool messages
  renders one line each, naming role + tool-call names + text.
- `maybe_compact_below_trigger_is_noop` — `last_input_tokens` under 85% of budget (or `None`) →
  `messages` unchanged, no trace event, provider never called.
- `maybe_compact_not_enough_history_is_noop` — trigger met but `completed.len() <= KEEP_LAST_TURNS`
  (fewer than 3 completed turns since the last fold) → unchanged, no trace event.
- `maybe_compact_folds_older_turns_keeps_recent_tail` — trigger met with 5 completed turns → exactly
  one `[compacted history]` message replaces the fold span (turns 0-2 folded); `HistoryCompacted`
  traced with the exact expected `(through_turn, dropped_turns, summary)` tuple; the last
  `KEEP_LAST_TURNS` (2) turns' messages are byte-identical and in original order.
- `maybe_compact_summarizer_failure_is_nonfatal` — `MockProvider` scripted to return empty text →
  `messages` unchanged, one `Event::Note` containing "compaction skipped" written, no
  `HistoryCompacted` event, `Ok(())` returned (not propagated as an error).
- `maybe_compact_repeat_fold_never_accumulates` — two triggered folds in sequence → `messages`
  contains exactly ONE `[compacted history]` message at any time (the second fold's summary
  supersedes the first, never sits alongside it) — the direct regression test for T-4002's most
  distinctive behavioral clause.
- `maybe_compact_trigger_boundary_is_exclusive_below` (test-critic C-003) — pins the exact
  85%-of-2800 boundary (2380.0) rather than only ever testing values comfortably clear of it:
  `tokens == 2379` stays a no-op, `tokens == 2380` triggers.

## T-4004 — `replay()` extension for `HistoryCompacted` (co-located `#[cfg(test)]` in `replay.rs`,
matching sprint 39's idiom)
- `replay_applies_one_history_compaction` — a synthetic trace with `HistoryCompacted` placed in its
  REAL shape (right after the triggering turn's own `TurnStart`, before its `TurnEnd` — matching
  `run.rs`'s wiring, not an artificially "obviously safe" position) reconstructs `messages` with the
  folded turns dropped, the summary inserted after the head, and the preserved tail
  byte-identical/ordered; `turns` reflects the TOTAL commit count (folded or not), not the reduced
  visible count.
- `replay_applies_two_sequential_history_compactions` — a SECOND `HistoryCompacted` later in the
  same trace folds the surviving tail from the first fold together with newly-eligible turns into
  ONE new summary; asserts the first fold's summary text does not linger anywhere in the
  reconstructed history.

## Result
`cargo test -p ferric-trace`: 12 passed (up from 11). `cargo test -p ferric-loop` (lib): 43 passed
(up from 34 — 7 new `compact::tests` incl. C-003's boundary test + 2 new `replay::tests`). `cargo
test -p ferric-loop` (integration, incl. `compaction_tests.rs`): all green. `--features
backend-openai`/`backend-mistralrs`: unaffected, both clean. See `critique.md` for the test-critic
pass and fixes (C-001 through C-004).
