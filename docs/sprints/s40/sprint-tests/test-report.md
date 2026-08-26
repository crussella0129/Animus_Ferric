# Sprint 40 Test Report — context-budget compaction

## Summary
All 6 build tasks (T-4001–T-4006) are covered by tests derived directly from `build-plan.md`'s
locked EARS clauses. A foreground test-critic agent independently re-derived the sprint's key
arithmetic (the exact `through_turn` value under the resumed-numbering fixture, the exact
`replayed.messages.len()` in the real compact→kill→replay→resume round-trip) and confirmed both
match the shipped implementation exactly. It surfaced 4 concerns (1 assertion-tightness gap, 1
documentation overclaim, 1 minor boundary-coverage gap, 1 benign non-issue) — all resolved inline
before finalizing. See `critique.md` for the full critique text and per-concern responses.

## Critic findings and resolutions
- **C-001** (tighten-assertion): `resume_seeds_compactor_numbering_consistently` asserted
  `through_turn >= 7` where the fixture's arithmetic is fully determinate (exactly 7) — a loose
  bound wouldn't catch an off-by-one regression shifting the result to 8/9/10. Tightened to
  `assert_eq!(through_turn, 7, ...)`.
- **C-002** (comment correction, test itself was already correct): `replay_applies_one_history_
  compaction`'s doc comment claimed the event's exact position (before its own turn's `TurnEnd`)
  was load-bearing for that test; tracing `replay()`'s real match arm shows it isn't (it operates
  solely on `committed_turn_starts`, advanced only via `TurnStart`). Corrected the comment to
  cross-reference the test that DOES verify the byte-order invariant end-to-end
  (`history_compacted_traced_after_triggering_turn_start`, against a real `run()`-produced trace).
- **C-003** (add-test, minor): no test pinned the exact 85%-of-budget trigger boundary (2380.0).
  Added `maybe_compact_trigger_boundary_is_exclusive_below`: `tokens == 2379` stays a no-op,
  `tokens == 2380` triggers.
- **C-004** (reject): confirmed non-issue — `resume_only_folds_new_post_resume_turns` is accurately
  described; no test-plan edit needed.

## Coverage by task
- **T-4001** (`Event::HistoryCompacted`): round-trip + full-vocabulary regression tests in
  `ferric-trace`. Shipped alongside T-4005's CLI rendering in the same commit (Rust's exhaustive
  `Event` matching forces both together — the same coupling sprint 39's T-3901 hit).
- **T-4002+T-4003** (`HistoryCompactor` + `run.rs` wiring): 7 co-located unit tests (below-trigger/
  not-enough-history no-ops, a successful fold's exact spliced shape, non-fatal summarizer failure,
  the repeat-fold-never-accumulates invariant, the exact trigger boundary) + 4 integration tests
  proving the real wiring (a real triggered fold shrinking `message_count` AND `chars`, the direct
  trace byte-order regression, the resume scope limit, the resume numbering consistency). Shipped
  as one commit — `HistoryCompactor`'s `pub(crate)` visibility trips dead-code analysis under `-D
  warnings` until `run.rs` calls it.
- **T-4004** (`replay()` extension): 2 synthetic-trace tests (one fold, two sequential folds) + the
  real compact→kill→replay→resume round-trip, independently verified arithmetically correct by the
  test-critic.
- **T-4005** (`ferric trace cat` legibility): 1 CLI subprocess test, verified against the real
  rendered output.
- **T-4006** (ADR-050 + docs): no test surface — verified by direct read during the critic pass.

## Final verification
- `cargo test --workspace`: all green (`ferric-trace` 12, `ferric-loop` lib 43, `ferric-loop`
  `compaction_tests` 5, `ferric-cli` `cli` 28, plus all other unaffected suites).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- `cargo build --workspace --features backend-openai` / `--features backend-mistralrs`: both clean.

## Confidence
Clean — proceed to Loop Phase.
