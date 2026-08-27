# Sprint 102 test plan — Finalized - DO NOT EDIT

The bug is latent, so the tests have to *create* the condition that would
expose it. A test that only runs at the default cap proves nothing here —
that is exactly why this survived since sprint 84.

## The contract test (the one that matters)

**`run()` under a non-default cap, then `replay()` of its real trace,
produces byte-identical messages.**

Drive a scripted run with `Registry::with_truncation_limit(N)` for some N far
from 4,000, with a tool output longer than N. Then replay the trace it wrote
and compare the reconstructed messages against the run's own. Today this
fails: replay re-truncates at 4,000. It must pass against a **real**
`run()`-produced trace, not a hand-built event list — a hand-built fixture
can't catch run and replay drifting apart, which is the whole failure mode.

Positive control in the same test file: the same comparison at the *default*
cap, which passes both before and after. Without it, a change that broke
replay outright would look like a pass.

## Unit tests

1. `PolicySelected` round-trips the new field through JSONL.
2. A `policy_selected` line **written before this sprint** (no
   `truncation_limit` key) deserializes to `DEFAULT_TRUNCATION_LIMIT` — the
   backward-compatibility claim, asserted against a literal old-format line
   rather than a re-serialized new one.
3. The projector picks the cap up from the event: step a `PolicySelected`
   carrying N, assert `projector.truncation_limit == N`.

## Regression surface

- Full `cargo test --workspace` — 569 tests must stay green; the event change
  touches 8 files, and the projector change touches every replay test.
- `cargo clippy --workspace --all-targets` at 0, `cargo fmt --check` clean.
- `ferric trace cat` on a real trace still prints, with the cap now shown.

## Not tested here, and why

`ferric trace verify` end-to-end under a non-default cap needs a trace
produced under one, which needs a surface that sets it — deliberately out of
scope above. The fix there is the same one-line source change as replay's and
is covered by the same event contract; the honest statement is that it is
verified by construction and not by execution.
