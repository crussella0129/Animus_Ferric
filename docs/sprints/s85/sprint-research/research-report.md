# Sprint 85 — Research Report

## 1. Goal

A second full-codebase verification, from a cold clean-room build.

## 2. The weighting decision

The obvious way to run round 2 is to repeat round 1 evenly across all 14 crates.
That would have been mostly wasted: sprints 83–84 rewrote much of what round 1
examined, so the *oldest* code is now the best-audited and the *newest* is
unexamined — and the newest was written by the same process now checking it.

So this round was weighted toward sprints 83–84's own output, with the rest of
the tree swept by defect **class** rather than file by file.

That decision produced the result: **3 of 4 new findings are regressions from the
remediation sprints**, and two sit in the security code those sprints were fixing.

## 3. Findings

Full report: `docs/verification-2026-07-round2.md`.

| # | Status | Origin | Summary |
|---|---|---|---|
| E1 | **PROVEN** | sprint 84 | One call prompts the human twice (accept-edits gate + sink gate). Measured `approver_prompt_count=2`. |
| E2 | **PROVEN** | sprint 83 | Taint granularity blocks 3/3 faithful restatements under the default `Deny`. **No threshold fixes it.** |
| E3 | CONFIRMED | sprint 84 | `run` seeds the projector cap from the registry; `replay`/`compact` use the default. Refutes ADR-074's own claim. |
| E4 | CONFIRMED | pre-existing | `chat.rs` discards trace-write failures at all 6 sites; `run.rs` propagates at 21. |

## 4. E2 is the one that matters, and it is not a bug

Substring taint tracking **cannot** distinguish an injected instruction the model
copied from a true fact the model learned — both are literal text derived from
the digest. Lowering `MIN_TAINT_SEGMENT_CHARS` worsens false positives; raising
it readmits the lifted sentence sprint 83 added `taint_text` to catch.

There is no value of the constant that works, so re-tuning it would have been the
wrong move — and a tempting one, since it looks like a one-line fix. It is
recorded as a posture decision with three costed options instead.

## 5. Classes swept clean

- **Panics** (the A4 class): 6 production `unwrap`/`expect` left across
  tools/loop/guard, all provably safe (constant regex, capture groups guaranteed
  by a successful match, `take()` immediately after `Stdio::piped()`).
- **Unresolved intent** (the A7 class): no `not wired`/TODO/FIXME left.
- **Stale ledger**: all seven pre-audit backlog items cite live files.

## 6. The bounding gap

Nothing has met a real model since ~sprint 26. All 503 tests are mock-driven;
A5's sandbox has never run against Docker; A2's and A6's thresholds are asserted
against synthetic inputs. **A suite this green, this long without a live run, is
measuring the mocks as much as the code.**

## 7. Process finding

C7, C8 and B1 came out of sprint 82 but were never entered in `agent-tasks/` —
they lived only in a README "Next" line, and went three sprints unpicked-up.
Prose is not a ledger. Entered now, along with E1–E4 and the live-model round.
