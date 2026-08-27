# Sprint 89 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **529 passed / 0 failed** (524 at start, +5) |
| clippy / fmt | 0 warnings / clean |

## E1 — one call, one prompt

| Test | Pins |
|---|---|
| `one_tool_call_prompts_the_human_once` | the regression (was 2 prompts) |
| `an_approved_tainted_write_actually_happens` | the carry-through is **real** — the approved call runs, rather than the prompt merely being suppressed and the call denied anyway |
| `without_an_approver_require_approval_still_denies` | nobody to ask ⇒ denial, unchanged |
| `the_preview_discloses_taint` | the merged prompt keeps the sink's information |
| `an_untainted_preview_has_no_warning` | the disclosure is conditional, not boilerplate |

The middle test is the important one: a fix that merely stopped the second prompt
while leaving the sink to deny afterwards would have passed a naive
"prompt count == 1" assertion and broken the feature.

## E4 — chat trace writes

All 6 discard sites converted to a `log_event` helper that warns once per
session. Verified by grep: `let _ = log.write_event` now appears **0** times.

## A note on the clippy result

Removing `dispatch`'s now-dead `edit_approver` parameter also took the function
back under the argument-count limit — the warning was fixed by deleting the dead
thing rather than by suppressing it.
