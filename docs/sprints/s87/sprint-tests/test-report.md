# Sprint 87 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **518 passed / 0 failed** (507 at sprint start, +11) |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `cargo fmt --all --check` | clean |

## F1 — the oscillation guard

6 unit + 5 integration tests. The integration scenario is the sprint-86 live run
reproduced verbatim (`search_files` / `find_files`, same args).

| Test | Pins |
|---|---|
| `a_two_cycle_is_stopped` | the live failure, at the unit level |
| `the_live_two_cycle_is_stopped_before_max_turns` | stop reason is `oscillation`, not `max_turns` |
| `it_stops_well_inside_the_turn_budget` | ≤10 requests, vs nano's 15-turn budget |
| `the_model_is_warned_before_being_stopped` | the warning reaches the model AND the trace |
| `genuine_progress_is_never_stopped` | **the false-positive boundary** — alternating tool *names* with fresh args is real work |
| `identical_repeats_still_report_as_repetition` | the sharper guards keep their cases; diagnostics don't degrade |
| `a_three_cycle_is_deliberately_not_caught` | `MAX_DISTINCT = 2` is a decision, not an accident |
| `one_alternation_is_fine`, `argument_order_does_not_matter`, `empty_turns_are_ignored` | edges |

Two of these were written wrong first and the failures were informative:
reading nonexistent files tripped `FailureGuard`, and eight same-name turns
tripped the pre-existing `ProgressGuard`. Both are correct behaviour of the other
guards — the test had to be sharpened to isolate *this* guard.

## Live validation

| Target | Before | After |
|---|---|---|
| F1 scenario | 20 turns, 2 distinct calls, **zero guards**, `max_turns` | **8 turns, warned ×2, `oscillation`** |
| A1 truncation | unexercised (model paginated) | trace **19,992 chars**, model ~4,000, model reported *"which has been truncated for display"* |

## G1 — new live finding

`ferric query --research "configuration"` over a workspace containing that word
injected **no** `<research_context>` (verified in the trace's `session_prompt`)
and printed nothing. `research_all` returns `Ok` with an empty digest list and
`query.rs` skips silently.

Consequence: **ADR-075's E2 taint finding remains unmeasured live** — no digest
means no taint means nothing to false-positive on. Said plainly rather than
letting a passing run imply the taint path was exercised.

## Not run

A5's sandbox (Docker absent), fleet re-calibration, and a weaker second model
(ZimaBoard2 share unreachable — see the research report).
