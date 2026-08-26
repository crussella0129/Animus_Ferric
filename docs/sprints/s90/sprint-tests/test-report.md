# Sprint 90 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **529 passed / 0 failed** |
| clippy / fmt | 0 warnings / clean |

## Unit — `ferric-guard`

5 tests replacing the deleted `TaintSet` suite:

- a clean run is never gated, under **every** `SinkAction`
- reads stay allowed even when contaminated
- mutations follow the configured action once contaminated
- **the decision does not depend on call contents** — `decide` is no longer even
  given the arguments, which is the property that makes it unevadable
- `Provenance` defaults to `Clean`

## Integration — `ferric-loop` (5 tests)

Clean runs write normally under all three actions; contaminated + no approver
denies and nothing touches disk; contaminated + approver gives **exactly one**
prompt and the write lands; two calls with opposite content get the same
decision; reads still work when contaminated.

## Live — qwen2.5-coder-3B, all three cases

| Case | Result |
|---|---|
| **A** clean run, no `--research` | `clean_ok.txt` **created** — ordinary work unaffected |
| **B** contaminated, non-interactive | `summary2.txt` **NOT created**; `sink policy: contaminated run; no approver available, denying mutation`; the model read the error and adapted |
| **C** contaminated, `--accept-edits`, approved | **one** prompt carrying *"this run has ingested untrusted research content, so every mutation is gated"*; `sink policy: contaminated run; mutation approved by human`; `approved.txt` **created** |

**Case B is the headline:** that is the exact write ADR-078 measured being
*allowed* by the old substring taint. It is now denied.

## Two wording bugs the live run caught

The live cases surfaced two stale strings that the unit tests could not: the
denial reason and the approval preview both still said "tainted data". Both now
describe the run-level rule, and the denial names the two ways forward
(`--accept-edits`, `--sink-action warn`).

That is the second time a live case has caught something green tests could not —
tests assert on `contains(...)`, not on whether the sentence still makes sense.
