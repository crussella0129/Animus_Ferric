# Sprint 40 Integration Tests

`crates/ferric-loop/tests/compaction_tests.rs` (new): real `run()`-driven scripted sessions against
`MockProvider`, plus one CLI subprocess test.

## T-4003 — `HistoryCompactor` wired into `run.rs`
- **Regression:** every pre-existing `ferric-loop`/`ferric-cli` test remains unaffected — no fixture
  crosses the 85%-of-budget trigger (`MockProvider` fixtures use small hand-set `input_tokens`,
  the primary proof this sprint didn't alter any existing session's behavior).
- `compaction_triggers_and_shrinks_next_prompt_assembled` — a scripted 5-turn session against
  `nano_policy()` (`prompt_budget_tokens == 2800`) with turn 3's `input_tokens` crossing 85%
  (2500 ≥ 2380); asserts a LATER turn's `Event::PromptAssembled.message_count` AND `.chars` are
  both smaller than the immediately-preceding turn's (the actual token-budget-relevant metric, not
  just message count — turns 0/1's assistant messages carry deliberately bulky prose text so their
  removal is measurable), and exactly one `HistoryCompacted` event appears in the trace.
- `history_compacted_traced_after_triggering_turn_start` — a direct regression test for the
  load-bearing ordering invariant itself (not just its downstream effect): opens the real written
  trace and asserts `TurnStart{turn: N}` appears strictly before `HistoryCompacted`, which appears
  strictly before `TurnEnd{turn: N}`, for the triggering turn.
- `resume_only_folds_new_post_resume_turns` — a resumed session (seeded `ReplayedState` with a
  pre-existing tool-call turn) drives new turns crossing the trigger; asserts the LAST request the
  provider saw still carries the ENTIRE replayed prefix byte-identical — it was never eligible for
  folding (the documented v1 scope limit).
- `resume_seeds_compactor_numbering_consistently` — a resumed session with `ReplayedState.turns ==
  7` drives new turns to a fold; asserts the traced `HistoryCompacted.through_turn == 7` EXACTLY
  (test-critic C-001 tightened this from a loose `>= 7` — the arithmetic is fully determinate, and
  a loose bound wouldn't catch an off-by-one shift to 8/9/10) — proves the absolute-turn-number
  redesign (no `turn_offset` accumulator) actually resolves the numbering concern rather than just
  relocating it.

## T-4005 — `ferric trace cat` legibility (`crates/ferric-cli/tests/cli.rs`)
- `trace_cat_renders_history_compacted` — a hand-written trace fixture containing a
  `history_compacted` line renders a line naming the folded-turn count, the through-turn, and a
  legible summary excerpt.

## Result
`cargo test -p ferric-loop --test compaction_tests`: 5 passed. `cargo test -p ferric-cli --test
cli`: 28 passed (up from 27). `cargo test --workspace`: all green. `cargo clippy --workspace
--all-targets` clean on default, `backend-openai`, and `backend-mistralrs` feature sets. `cargo fmt
--all --check` clean. See `critique.md` for the test-critic pass and fixes (C-001 through C-004).
