# Sprint 95 — Research Report

## 1. Goal

Refresh the fleet capability map, untouched since sprints 25–26, now that two
same-family models are local.

## 2. Why same-family mattered

`Qwen2.5-Coder` at 3B and 7B holds tuning, tokenizer and prompt-format constant,
so the difference in `measured_level` is attributable to **size** rather than to
model idiosyncrasy. 3B tops out at L4; 7B completes the ladder.

## 3. The re-calibration validated the old number

The 7B measured 6 in sprint 20. It measures 6 now, after the guard family, the
truncation fix, the projector refactor, the provenance gate and much else. That
is a useful negative result: none of it moved the capability ceiling, which is
what those ADRs claimed ("bounds wasted compute, does not lift a ceiling").

## 4. Two defects, both the same shape

**H1** — `calibrate()` runs on the current sweep's rows only, so a `--level N`
diagnostic overwrote a full sweep's profile, downgrading the tier that
`ferric query` then uses.

**H2** — `measured_level = max(passed)` silently flattened a sweep that failed L5
and passed L6.

Both are *a value computed from partial information, reported as the whole
picture* — the recurring shape of this whole series. Neither was reachable from
the test suite; both needed a real ladder against a real model.

## 5. On H2's fix

The temptation was to change the formula to "level before first failure". That
would have been wrong: repeats showed L5 was noise, and the conservative formula
would have scored the 7B at 4 — equal to the 3B, contradicting the plain evidence
that it completes L6.

The problem is not the formula, it is the **single sample** and the silence about
inconsistency. So the fix reports rather than re-derives, and says explicitly
that the figure should be re-run before it is trusted.
