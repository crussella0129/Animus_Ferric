# Sprint 85 Meta

- **Sprint number:** 85
- **Start timestamp:** 2026-07-25T04:10:53Z
- **End timestamp:** 2026-07-25T05:05:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Second full-codebase verification from a cold clean-room build,
  weighted toward sprints 83–84's own changes.

## Outcome

Baseline clean-room green: 0-warning cold build (41s), **503 passed / 0 failed /
2 ignored** across 53 suites, clippy 0, fmt clean, DM verifier 62/0/0.

Four new findings — **three of them regressions introduced by sprints 83–84**,
two of those in the security-facing code those sprints existed to fix:

- **E1 (PROVEN)** one tool call prompts the human twice — accept-edits gate and
  sink gate both fire. Measured `approver_prompt_count=2`.
- **E2 (PROVEN)** the sprint-83 taint granularity blocks 3/3 faithful
  restatements of researched material under the default `Deny`. **No threshold
  fixes it** — substring taint cannot separate a copied injection from a learned
  fact. A posture decision, not a patch.
- **E3 (CONFIRMED)** `run` and `replay` disagree about the truncation cap,
  refuting a claim ADR-074 itself made.
- **E4 (CONFIRMED, pre-existing)** `ferric chat` discards trace-write failures at
  all 6 sites; `run.rs` propagates at 21.

Three defect **classes** closed: production panics down to 6 provably-safe
idioms, no unresolved intent comments, no stale ledger entries.

## The weighting call

Repeating round 1 evenly would have re-audited the best-audited code and skipped
the newest. Weighting toward our own recent output is what surfaced E1–E3.

## Bounding gap

Nothing has met a real model since ~sprint 26. All 503 tests are mock-driven, the
sandbox has never run against Docker, and both tuning constants are asserted
against synthetic inputs. **A live-model round is worth more than C7/C8/B1
combined.**

## Process finding

C7/C8/B1 came out of sprint 82 but were never entered in `agent-tasks/` — they
lived only in a README "Next" line and went three sprints unpicked-up. Prose is
not a ledger.

## Scope

Audit only; the tree is unmodified apart from four documents. Probes run,
recorded, deleted; suite green afterwards.
