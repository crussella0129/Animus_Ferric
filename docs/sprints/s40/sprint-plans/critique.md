# Plan Critique — Sprint 40

Reviewed by a foreground plan-critic agent against `research-report.md`, `decisions.md`, and the
real current source (`run.rs`, `replay.rs`, `scale.rs`, `message.rs`, `backoff.rs`, `event.rs`).

## C-001: `replay.rs`'s real `TurnStart{ .. }` match arm discards the turn number entirely
- **Finding:** the plan's `turn_starts: Vec<(u32, usize)>` extension assumed turn numbers were
  already in scope during replay's commit walk. They aren't — `PendingTurn` has no `turn` field
  and the real code pattern-discards `TurnStart{ .. }`.
- **Response:** **fix-in-plan.** Redesigned (see Design Corrections below): `PendingTurn` gains a
  `turn: u32` field, captured from the `TurnStart{turn}` event that opens it; commit pushes
  `(turn, messages.len())` into a new `committed_turn_starts: Vec<(u32, usize)>` before extending
  `messages`. This is real, non-trivial new plumbing — now stated explicitly in the plan rather
  than implied to "drop in for free."

## C-002: the ordering invariant is a coding convention, not something the design structurally proves
- **Finding:** "safe by construction" overstated what's enforced — nothing stops a future refactor
  from calling `maybe_compact` before `record_turn_start`, silently shifting the fold boundary by
  one turn.
- **Response:** **fix-in-plan.** The redesign (below) makes the bound explicit AND structural: the
  just-pushed CURRENT turn's `(turn, idx)` entry is excluded via a `[..len-1]` slice before any
  fold-span math runs, so `through_turn` can never include the turn that's still in flight — as
  long as `record_turn_start` for the current turn runs before `maybe_compact`, which is now stated
  as an explicit precondition. C-007 adds a direct regression test for the trace byte-order itself
  (not just the downstream math), closing the "convention, not enforcement" gap for good.

## C-003: `turn_offset`'s role and update formula were asserted, never derived
- **Finding:** the build-plan named a `turn_offset` field but never showed why it's needed or the
  formula it must satisfy — flagged as the single riskiest unverified arithmetic in the sprint.
- **Response:** **fix-in-plan — and the field is eliminated entirely.** Storing ABSOLUTE turn
  numbers directly in `turn_starts: Vec<(u32, usize)>` (both in `HistoryCompactor` and in
  `replay()`'s walk) removes the need for any offset/re-keying scheme — `through_turn` is read
  directly off the tracked pairs (`completed[fold_count - 1].0`), not computed from an accumulator.
  Simpler AND removes the ambiguity C-003 flagged.

## C-004: the fold-span boundary (`fold_from`/`fold_to`) was never given a closed form
- **Finding:** no formula proved `turn_starts[fold_count]` actually lands on "start of the first
  preserved turn" rather than off-by-one on the last folded turn.
- **Response:** **fix-in-plan.** With `completed = &turn_starts[..len-1]` (completed turns only,
  current excluded) and `fold_count = completed.len() - KEEP_LAST_TURNS`: `fold_to_idx =
  completed[fold_count].1` is *exactly* "start index of the first entry beyond the folded range,"
  by construction of the slice split — no off-by-one ambiguity remains once expressed this way
  (see Design Corrections).

## C-005: T-4004's blanket "Depends on T-4003" over-states what the synthetic-trace tests need
- **Finding:** every existing `replay.rs` test hand-authors trace events directly; only the ONE
  real-run end-to-end test in T-4004 actually needs T-4003's `run.rs` wiring to exist.
- **Response:** **fix-in-plan.** Dependency rationale corrected in build-plan.md: T-4004's parsing/
  splicing logic and its synthetic-trace tests depend only on T-4001 (the event shape); only the
  final real-run-then-replay-then-resume test genuinely needs T-4003. Kept as one task (matches
  elementary-task granularity for "extend replay() for HistoryCompacted" as a single coherent
  diff), but the stated rationale no longer overclaims.

## C-006: resume + compaction turn-numbering interaction
- **Finding:** on a resumed session, `turns` seeds from a nonzero `replayed.turns`; the critic asked
  whether `HistoryCompactor`'s internal numbering (via the now-removed `turn_offset`) would stay
  consistent with that.
- **Response:** **fix-in-plan — resolved by the C-003 redesign.** Since `turn_starts` now tracks
  ABSOLUTE turn numbers directly (not a zero-based relative scheme with an offset), a resumed
  session's `HistoryCompactor` naturally starts recording from whatever `turns` the loop is
  actually at (e.g. 7) with zero special-casing — `record_turn_start(7, ...)` just works. The
  documented v1 scope limit (only NEW post-resume turns are foldable, the replayed prefix never is)
  remains intentional and unrelated to numbering — it's enforced by `head_len` covering the entire
  seeded history, not by turn-number bookkeeping. Added `resume_seeds_compactor_numbering_
  consistently` test (see Test Plan Additions) proving new post-resume turn numbers land correctly.

## C-007: no test asserts the load-bearing `TurnStart`-before-`HistoryCompacted` byte order in a real trace file
- **Response:** **fix-in-plan.** Added `history_compacted_traced_after_triggering_turn_start` to
  T-4003's integration tests: opens the real written trace after a triggered fold and asserts via
  `TraceReader` that `TurnStart{turn: N}` appears strictly before `HistoryCompacted`, which appears
  strictly before `TurnEnd{turn: N}`, for the triggering turn.

## C-008: `complete_with_backoff`'s retry latency on a failed compaction attempt (up to ~1.75s) isn't mentioned
- **Response:** **defer-with-rationale.** Not a correctness defect — `ProviderError` is a normal
  `Result::Err` `maybe_compact` catches cleanly into `Ok(())` + a `Note`, exactly as planned. The
  latency cost is real but is an accepted cost of reusing the existing retry policy rather than
  inventing a second one; noted explicitly in ADR-050 rather than blocking the plan.

## C-009: the "pathological large fold" backstop deferral should be cross-referenced, not left only in research-report.md
- **Response:** **fix-in-plan (minor).** T-4006's EARS clause already lists "chunked summarization
  for pathologically large folds" and "a hard truncation backstop" among explicit deferrals —
  confirmed this stays in the locked build-plan text so it doesn't evaporate between phases.

## C-010: only `message_count` shrinking is tested, not `chars` — the actual token-budget-relevant metric
- **Response:** **fix-in-plan.** Added an assertion to `compaction_triggers_and_shrinks_next_
  prompt_assembled` (T-4003 integration test) that the post-fold turn's `Event::PromptAssembled.
  chars` is also smaller than the pre-fold trajectory would have produced, not just
  `message_count`.

## Design Corrections (folded into build-plan.md before lock)
The `turn_offset` field is REMOVED from `HistoryCompactor`. Both `HistoryCompactor` (run.rs side)
and `replay()`'s walk (replay.rs side) now track `Vec<(u32 absolute_turn_number, usize
start_index_in_messages)>` directly — no relative/offset scheme. This is simpler than the
originally-planned design AND resolves C-001, C-003, C-004, and C-006 simultaneously. See the
revised T-4002/T-4003/T-4004 task text in `build-plan.md`.

## Confidence
proceed-with-caveats → all 10 concerns addressed inline above (8 fix-in-plan, 1 defer-with-
rationale, 1 minor confirm-only); build-plan.md and test-plan.md revised accordingly before lock.
