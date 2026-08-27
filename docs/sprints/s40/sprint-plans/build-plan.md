Finalized - DO NOT EDIT

# Sprint 40 Build Plan

## Schema Tree
- Sprint Goal: context-budget compaction — enforce `RunPolicy.prompt_budget_tokens` via
  model-driven summarization of older turns
  - Trace format
    - T-4001: `Event::HistoryCompacted` new trace variant
  - Compaction mechanism
    - T-4002: `crates/ferric-loop/src/compact.rs` — `HistoryCompactor` + pure helpers
    - T-4003: Wire `HistoryCompactor` into `run.rs`
  - Resume interaction (cross-cutting, required not optional)
    - T-4004: Extend `replay()` for `HistoryCompacted`
  - Legibility + docs
    - T-4005: `ferric trace cat` legibility
    - T-4006: ADR-050 + docs

## Execution Sequence

### T-4001: `Event::HistoryCompacted` new trace variant
- **Touches:** `crates/ferric-trace/src/event.rs`, `crates/ferric-trace/src/lib.rs`
- **Depends on:** (none)
- New, purely additive `Event` variant: `HistoryCompacted { through_turn: u32, dropped_turns: u32,
  summary: String }`. No `#[serde(default...)]` needed (a brand-new variant, not an extension of an
  existing one — old readers already tolerate unknown variants per ADR-002).
- **Success criterion (EARS):**
  - **WHEN** an `Event::HistoryCompacted` value is serialized and deserialized, **THEN** the
    round-trip **SHALL** preserve `through_turn`, `dropped_turns`, and `summary` exactly.
  - **WHEN** `jsonl_roundtrip_all_event_types` is extended with one `HistoryCompacted` entry,
    **THEN** the full vocabulary (old + new) **SHALL** still round-trip as one set (regression).

### T-4002: `crates/ferric-loop/src/compact.rs` (new file) — `HistoryCompactor` + pure helpers
- **Touches:** new file `crates/ferric-loop/src/compact.rs`; `crates/ferric-loop/src/lib.rs`
  (module registration, `pub(crate)` only — no public API surface)
- **Depends on:** T-4001
- **Revised after plan-critic C-001/C-003/C-004** (see `critique.md`): `turn_starts` tracks
  ABSOLUTE turn numbers directly, not a relative/offset scheme — this removes an earlier
  `turn_offset` field entirely and makes the fold-span boundary a closed form, not an assertion.
  ```rust
  pub(crate) struct HistoryCompactor {
      head_len: usize,
      /// (absolute turn number, start index in `messages`) for every turn
      /// completed SINCE the last fold, in order — including the just-started
      /// CURRENT turn's own entry (always the last one, always excluded from
      /// folding; see `maybe_compact`'s `completed` slice below).
      turn_starts: Vec<(u32, usize)>,
  }
  ```
  Fixed constants: `COMPACT_TRIGGER_FRACTION = 0.85`, `KEEP_LAST_TURNS = 2` (v1 scope: not
  per-tier `RunPolicy` fields — documented in ADR-050).
  `record_turn_start(&mut self, turn: u32, messages_len: usize)` — pushes `(turn, messages_len)`.
  `render_transcript(messages: &[Message]) -> String` (pure fn: one line per message naming role,
  any tool-call names, and text).
  `async fn maybe_compact(&mut self, provider, sleeper, sink, policy, messages,
  last_input_tokens) -> Result<(), FerricError>`:
  1. No-op if `last_input_tokens` is `None` or below the trigger fraction.
  2. `let completed = &self.turn_starts[..self.turn_starts.len() - 1];` — **excludes the
     just-started current turn's own entry** (always the last pushed); this is the structural
     mechanism (not just a call-order convention) that guarantees the current turn can never be
     folded (closes plan-critic C-002). No-op if `completed.len() <= KEEP_LAST_TURNS`.
  3. `let fold_count = completed.len() - KEEP_LAST_TURNS;` `let (through_turn, _) =
     completed[fold_count - 1];` (read directly off the tracked pair — no derivation needed).
     `let fold_from_idx = self.head_len;` `let fold_to_idx = completed[fold_count].1;` — by
     construction of the slice split, this is exactly "the start index of the first entry beyond
     the folded range," closing plan-critic C-004's off-by-one concern.
  4. `render_transcript(&messages[fold_from_idx..fold_to_idx])` (includes a PRIOR compaction
     summary message if one already sits at `head_len` — folding it together with newly-eligible
     turns into one NEW summary is how repeat compactions stay to a single summary message).
  5. `summarize_history(provider, sleeper, transcript)` — single-shot, no tools, unconstrained
     free-text completion via the EXISTING `crate::backoff::complete_with_backoff` (reuses the
     established retry policy; accepted cost — a retryable failure costs up to ~1.75s of backoff
     sleep before giving up, noted in ADR-050 per plan-critic C-008, not blocking). Reuses the SAME
     provider as the main loop (no second, cheaper model exists in Ferric's one-local-model
     architecture).
  6. On provider error or empty output: write one `Event::Note` ("compaction skipped: ..."),
     change nothing, return `Ok(())` — non-fatal.
  7. On success: `sink.write_event(Event::HistoryCompacted { through_turn, dropped_turns:
     fold_count as u32, summary })?`; `messages.splice(fold_from_idx..fold_to_idx,
     [Message::user(format!("[compacted history] {summary}"))]);` shift and keep only the
     surviving (preserved-tail) entries in `turn_starts`, at their new (shifted) indices — their
     ABSOLUTE turn numbers are unchanged (no re-keying needed, since numbers were never relative).
- **Success criterion (EARS):**
  - **WHEN** `maybe_compact` is called with `last_input_tokens` below `COMPACT_TRIGGER_FRACTION *
    policy.prompt_budget_tokens` (or `None`), **THEN** it **SHALL** leave `messages` unchanged and
    write no trace event.
  - **WHEN** the trigger fraction is met but `completed.len() <= KEEP_LAST_TURNS` (fewer than
    `KEEP_LAST_TURNS + 1` turns completed since the last fold), **THEN** it **SHALL** leave
    `messages` unchanged and write no trace event.
  - **WHEN** the trigger fires with enough history, **THEN** it **SHALL** replace exactly
    `messages[fold_from_idx..fold_to_idx]` with one `Message::user("[compacted history] ...")`,
    trace one `Event::HistoryCompacted` whose `through_turn` equals `completed[fold_count-1].0`,
    and leave the most recent `KEEP_LAST_TURNS` turns' messages byte-identical and in order.
  - **WHEN** the summarization completion fails (provider error) or returns empty/absent text,
    **THEN** `maybe_compact` **SHALL** leave `messages` unchanged, write one `Event::Note`
    describing the skip, and return `Ok(())` (non-fatal).
  - **WHEN** `maybe_compact` triggers a SECOND fold after an earlier one, **THEN** it **SHALL** fold
    the prior summary message together with newly-eligible turns into ONE new summary message
    (never accumulate multiple summary messages in `messages`).

### T-4003: Wire `HistoryCompactor` into `run.rs`
- **Touches:** `crates/ferric-loop/src/run.rs`
- **Depends on:** T-4002
- Construct `HistoryCompactor::new(head_len)` once at loop start, where `head_len` = `messages.len()`
  right after the existing fresh/resume seeding block (uniform for both cases — no branching
  needed; on a resumed session `head_len` covers the ENTIRE replayed history, so only NEW turns
  generated after resuming are foldable — a deliberate, documented v1 scope limit, unrelated to
  turn-number bookkeeping since numbers are tracked absolutely, not relatively — see T-4002's
  design correction and plan-critic C-006). Track `last_input_tokens: Option<u32>`, updated right
  after `Event::TurnEnd` is written each turn. At the TOP of the next iteration, right after
  `sink.write_event(Event::TurnStart{turn})?`: call `compactor.record_turn_start(turn,
  messages.len())` then `compactor.maybe_compact(...).await?`. **Ordering is load-bearing and now
  structurally enforced, not just conventional** (plan-critic C-002): `record_turn_start` for the
  CURRENT turn must run before `maybe_compact`, so `HistoryCompactor`'s internal `completed` slice
  (T-4002) always excludes the in-flight turn. C-007 adds a direct test asserting the real trace
  byte-order (`TurnStart` before `HistoryCompacted` before that turn's own `TurnEnd`), not just the
  downstream message-count effect.
- **Success criterion (EARS):**
  - **WHEN** a session's `input_tokens` never approaches `prompt_budget_tokens`, **THEN** `run()`'s
    behavior **SHALL** be byte-identical to before this sprint (regression — full existing test
    suite unchanged, no new `RunArgs` field, no CLI flag).
  - **WHEN** a session's `input_tokens` crosses the trigger fraction, **THEN** the NEXT turn's
    `Event::PromptAssembled.message_count` AND `.chars` **SHALL** both reflect the folded (smaller)
    message set (plan-critic C-010 — `chars` is the actual token-budget-relevant metric, not just
    message count).
  - **WHEN** the trace is inspected after a triggered fold, **THEN** `TurnStart{turn: N}` **SHALL**
    appear strictly before `HistoryCompacted`, which **SHALL** appear strictly before
    `TurnEnd{turn: N}`, for the triggering turn N (plan-critic C-007 — a direct regression test for
    the load-bearing ordering, not just its downstream effect).
  - **WHEN** `args.resume` is `Some` and NEW turns generated after resuming cross the trigger,
    **THEN** those new turns **SHALL** fold correctly with absolute turn numbers consistent with
    the loop's own (possibly nonzero) `turns` counter, while the entire replayed prefix stays
    unfoldable (plan-critic C-006).

### T-4004: Extend `replay()` for `HistoryCompacted`
- **Touches:** `crates/ferric-loop/src/replay.rs`
- **Depends on:** T-4001 for the synthetic-trace parsing/splicing logic and its unit tests (every
  existing `replay.rs` test hand-authors trace events directly via `write_trace`, never calls
  `run()` — this task's core logic is no different); T-4003 ONLY for the final real-run-then-
  replay-then-resume end-to-end test, which needs a real triggered fold to exist. (Plan-critic
  C-005 — the blanket "depends on T-4003" over-stated what the bulk of this task actually needs.)
- **Revised after plan-critic C-001** (see `critique.md`): the real `replay.rs` code today
  pattern-discards `TurnStart{ .. }`'s own `turn` field entirely (`ParsedEvent::Known(Event::
  TurnStart { .. })`) and `PendingTurn` has no `turn` field — so this task requires real new
  plumbing, not a drop-in extension:
  - `PendingTurn` gains a `turn: u32` field, captured when it's opened: `pending = Some(PendingTurn
    { turn, ..Default::default() })` (from the `TurnStart{ turn }` event, no longer discarded).
  - A new `committed_turn_starts: Vec<(u32, usize)>` is pushed inside `commit_and_reset!()`, BEFORE
    `messages.extend(msgs)`: `committed_turn_starts.push((p.turn, messages.len()))` — the turn
    being committed there is the one that was PENDING before the incoming `TurnStart` (i.e.
    whichever turn opened it), never the just-arrived one.
  - `head_len` is tracked (`messages.len()` right after the `SessionPrompt` arm pushes
    `[system, user]`) — needed as the fold's fixed truncation point.
  - On `Event::HistoryCompacted { through_turn, summary, .. }` (processed as its own match arm,
    AFTER the `TurnStart(current)` arm that opened the fresh, still-empty `pending` for the
    current turn — never touching it, matching T-4003's structural exclusion):
    `split_at = committed_turn_starts.partition_point(|&(t, _)| t <= through_turn);` if `split_at >
    0`: save `preserved_tail = messages[preserve_from_idx..].to_vec()` (where `preserve_from_idx =
    committed_turn_starts.get(split_at).map(|&(_, i)| i).unwrap_or(messages.len())`); truncate
    `messages` to `head_len`; push the summary message; re-append `preserved_tail`; rebuild
    `committed_turn_starts` from `committed_turn_starts[split_at..]`, shifted to the new indices —
    ABSOLUTE turn numbers stay unchanged (no re-keying, mirrors T-4002's correction).
  - Handles repeat compactions naturally: a second `HistoryCompacted` later just re-partitions
    whatever `committed_turn_starts` currently holds.
- **Success criterion (EARS):**
  - **WHEN** a trace contains one `HistoryCompacted{through_turn, summary}` event, **THEN**
    `replay()`'s reconstructed `messages` **SHALL** drop every turn numbered `<= through_turn`,
    insert one `Message::user("[compacted history] {summary}")` immediately after the head, and
    preserve every turn numbered `> through_turn` byte-identical and in order.
  - **WHEN** a trace contains TWO `HistoryCompacted` events, **THEN** `replay()` **SHALL** apply
    both in order, ending with exactly one summary message reflecting the LATEST fold.
  - **WHEN** a real `run()` session compacts, is killed (trace truncated), and is
    `replay()`-then-resumed, **THEN** the resumed session's reconstructed history **SHALL** be
    smaller than the full pre-compaction history (the end-to-end proof the mechanism achieves its
    purpose across a resume boundary).
  - **WHEN** a resumed session (nonzero starting `turns`) generates new turns that are later
    folded, **THEN** the resulting `HistoryCompacted.through_turn` **SHALL** use the SAME absolute
    turn numbering the loop itself used (plan-critic C-006 — proves the elimination of the
    relative/offset scheme didn't just move the bug, it removed it).

### T-4005: `ferric trace cat` legibility
- **Touches:** `crates/ferric-cli/src/trace_cmd.rs`
- **Depends on:** T-4001
- **Success criterion (EARS):**
  - **WHEN** `ferric trace cat` renders a `HistoryCompacted` event, **THEN** it **SHALL** show the
    folded-turn count and a legible excerpt of the summary text.

### T-4006: ADR-050 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-4001–T-4005
- **Success criterion (EARS):**
  - **WHEN** ADR-050 is read, **THEN** it **SHALL** state the trigger design (fixed constants and
    why they're not per-tier config for v1), the required `replay()` extension and why it is not
    optional, the same-provider-reuse constraint (no second cheaper model), the non-fatal-failure
    decision (including the accepted ~1.75s worst-case backoff latency on a failed compaction
    attempt, plan-critic C-008), the resume-interaction scope limit, the absolute-turn-numbering
    design correction made during plan critique (removing the originally-planned `turn_offset`
    field), and explicit deferrals (per-tier tuning, chunked summarization for pathologically large
    folds, a hard truncation backstop).
