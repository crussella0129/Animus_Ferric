Finalized - DO NOT EDIT

# Sprint 40 Test Plan

## Unit Tests

### T-4001 unit tests
- `history_compacted_roundtrip`: `through_turn`/`dropped_turns`/`summary` survive
  serialize→deserialize exactly.
- `jsonl_roundtrip_all_event_types` extended with one `HistoryCompacted` entry — full vocabulary
  (old + new) still round-trips as one set.

### T-4002 unit tests
- `render_transcript_names_roles_and_tool_calls`: a slice of system/user/assistant/tool messages
  renders one line each, naming role + tool-call names + text.
- `maybe_compact_below_trigger_is_noop`: `last_input_tokens` under the fraction (or `None`) →
  `messages` unchanged, no trace event.
- `maybe_compact_not_enough_history_is_noop`: trigger met but fewer than `KEEP_LAST_TURNS + 1`
  completed turns → unchanged, no trace event.
- `maybe_compact_folds_older_turns_keeps_recent_tail`: trigger met with enough history → exactly one
  `[compacted history]` message replaces the fold span; `HistoryCompacted` traced with correct
  `through_turn`/`dropped_turns`; the last `KEEP_LAST_TURNS` turns' messages are byte-identical and
  in original order.
- `maybe_compact_summarizer_failure_is_nonfatal`: `MockProvider` scripted to error → `messages`
  unchanged, one `Event::Note` written, `Ok(())` returned (not propagated as an error).
- `maybe_compact_repeat_fold_never_accumulates`: two triggered folds in sequence → `messages`
  contains exactly ONE `[compacted history]` message at any time (the second fold's summary
  supersedes the first, it does not sit alongside it).
- Stubs: `MockProvider` (existing `ferric-provider` test helper).

## Integration Tests

### T-4003 integration (`crates/ferric-loop` test suite)
- **Regression:** every pre-existing `ferric-loop`/`ferric-cli` test remains unaffected (no fixture
  crosses the 85%-of-budget trigger — the primary proof this sprint didn't alter any existing
  session's behavior).
- `compaction_triggers_and_shrinks_next_prompt_assembled`: a scripted multi-turn session against
  `nano_policy()` (`prompt_budget_tokens == 2800`) with `input_tokens` crossing 85% (≥2380) after
  enough turns to clear `KEEP_LAST_TURNS`; asserts a LATER turn's `Event::PromptAssembled.
  message_count` **AND `.chars`** are both smaller than they would be without the fold (plan-critic
  C-010 — `chars` is the metric that actually reflects token-budget pressure; message count alone
  is only a proxy and could mislead if the summary itself is verbose), and exactly one
  `HistoryCompacted` event appears in the trace at the expected turn.
- `history_compacted_traced_after_triggering_turn_start` (plan-critic C-007): after a triggered
  fold, opens the real written trace via `TraceReader` and asserts the literal event order for the
  triggering turn N: `TurnStart{turn: N}` strictly before `HistoryCompacted`, strictly before
  `TurnEnd{turn: N}` — a direct regression test for the ordering invariant T-4003/T-4004's
  correctness depends on, not just its downstream message-count effect.
- `resume_only_folds_new_post_resume_turns`: seed a resumed session (via T-3904's `resume: Some`
  path) then script enough new turns to cross the trigger; assert the fold's span excludes the
  entire replayed prefix (the documented v1 scope limit) while still folding the new turns.
- `resume_seeds_compactor_numbering_consistently` (plan-critic C-006): resume from a
  `ReplayedState` with a NONZERO `turns` (e.g. 7), script enough new turns to trigger a fold, and
  assert the resulting `HistoryCompacted.through_turn` uses the SAME absolute turn numbers the
  loop's own `turns` counter assigned (e.g. folds turns 7-8, not 0-1) — proves the C-001/C-003
  redesign (absolute, not relative, turn tracking) actually resolves the numbering concern rather
  than just relocating it.

### T-4004 integration (`replay.rs` co-located `#[cfg(test)]`, matching sprint 39's idiom)
Per plan-critic C-005: the first two tests below are hand-authored synthetic traces (via the
existing `write_trace` helper) and depend only on T-4001's event shape, matching every other
`replay.rs` test's idiom — they do NOT need T-4003's `run.rs` wiring to exist. Only the third test
is a genuine end-to-end proof and needs T-4003.
- `replay_applies_one_history_compaction`: a synthetic trace with one `HistoryCompacted` event
  reconstructs `messages` with the folded turns dropped, the summary inserted after the head, and
  the preserved tail byte-identical/ordered.
- `replay_applies_two_sequential_history_compactions`: a synthetic trace with TWO
  `HistoryCompacted` events (in order) ends with exactly one summary message reflecting the LATEST
  fold, and the correct surviving tail.
- `real_run_compact_kill_replay_resume_shrinks_history` (the strongest test, mirrors sprint 39's
  `real_run_then_replay_then_resume_reaches_task_complete`; needs T-4003): a REAL `run()` call
  scripted to trigger a real compaction, its REAL trace file truncated (drop the trailing
  `session_end` to simulate a kill), `replay()`d, and the resulting `ReplayedState.messages`
  asserted SMALLER than the pre-compaction full history would have been — the end-to-end proof the
  mechanism survives a resume boundary.

### T-4005 integration (`crates/ferric-cli/tests/cli.rs`)
- `trace_cat_renders_history_compacted`: a hand-written trace fixture containing a
  `history_compacted` line renders a line naming the folded-turn count and a legible summary
  excerpt.

## End-to-End Tests
- **Status:** possible (via `--mock`, no real GGUF model required) — covered by
  `real_run_compact_kill_replay_resume_shrinks_history` above (filed under the T-4004 integration
  section per sprint 38/39's precedent of not duplicating a test across files just to satisfy a
  section heading; it is the strongest, most end-to-end proof in the suite).
- A real interrupted-process scenario (`kill -9` a live `ferric query` mid-run against a REAL
  backend, after real compaction fired, then `--resume` it) remains a **manual verification step**,
  not automated — matches the project's established no-live-backend-CI position (ADR-045).

## Build/Lint (all tasks)
`cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt
--all --check`; `--features backend-openai`/`--features backend-mistralrs` builds unaffected.
