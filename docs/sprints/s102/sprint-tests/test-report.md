# Sprint 102 test report

**569 → 574 tests, 0 failures. clippy 0. `cargo fmt --check` clean.**

## New tests (5)

| test | crate | what it pins |
|---|---|---|
| `replay_rebuilds_the_same_context_window_the_run_used_at_a_non_default_cap` | ferric-loop | the contract: a real `run()` at cap 500, its trace killed and replayed, must rebuild the exact messages the model saw |
| `replay_rebuilds_the_same_context_window_the_run_used_at_the_default_cap` | ferric-loop | positive control — the same equality at the default, true before and after |
| `replay_takes_the_truncation_cap_from_the_trace` | ferric-loop | the projector reads the cap from `PolicySelected` |
| `a_pre_adr_093_policy_line_reads_back_at_the_default_cap` | ferric-trace | backward compatibility, asserted against a literal old-format line |
| `trace_verify_finds_no_drift_in_a_real_trace` | ferric-cli | `ferric trace verify` against a `run()`-produced trace carrying tool calls |

## Both fixes were confirmed by reverting them

Green tests prove nothing here on their own — the defects were latent, so the
suite was already green *with* them.

- Cap fix reverted → `replay_rebuilds_..._non_default_cap` fails.
- `trace verify` fix reverted → `trace_verify_finds_no_drift_in_a_real_trace`
  fails with `Mismatch in number of events: 12 vs golden 16`.

**The first revert attempt was wrong, and that matters.** Disabling the
projector's new behaviour alone broke run *and* replay symmetrically: both fell
back to the default, so they still agreed, the headline equality **passed**,
and only the secondary length assertion caught anything. A faithful
reproduction needed run's registry-seeded cap restored while replay stayed on
the default. **A revert that breaks both sides of an equality does not test the
equality.**

## Also exercised live

- `ferric trace cat` on a **real pre-sprint-102 trace** whose `policy_selected`
  line genuinely lacks the new key (verified by grepping the file): reads back
  `tool output cap 4000`. Backward compatibility measured against on-disk data,
  not a re-serialized fixture.
- A fresh `ferric query --mock` run: the new key is present and populated.
- `ferric trace verify` on that fresh trace: **`Trace verification successful.`**
  It reported `Mismatch in number of events: 16 vs golden 14` before the fix.

## Not covered, stated plainly

`ferric trace verify` under a **non-default** cap. Producing such a trace needs
a surface that sets the cap, which this sprint deliberately did not add. That
path is verified by construction — it reads the same event through the same
constant — and not by execution.
