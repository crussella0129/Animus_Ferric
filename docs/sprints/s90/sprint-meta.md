# Sprint 90 Meta

- **Sprint number:** 90
- **Start timestamp:** 2026-07-25T13:24:26Z
- **End timestamp:** 2026-07-25T14:20:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Resolve E2 — replace substring taint with a structural
  run-provenance gate (Option 4, approval form).

## Outcome

**529 tests, 0 failures**; clippy 0; fmt clean.

Substring taint is retired. `ferric_guard::Provenance { Clean,
UntrustedIngested }` replaces `TaintSet`, and `SinkPolicy::decide` takes it in
place of a per-call `tainted` bool. **`decide` is no longer even given the
arguments** — which is the property that makes the gate unevadable, because
nothing is being detected.

Default is now `RequireApproval`. A clean run is **never** gated; a contaminated
one asks a human once per mutation (via ADR-079's merged prompt) and denies when
nobody can answer.

## Validated live (qwen2.5-coder-3B)

- **Clean run** → writes normally. Ordinary work untouched.
- **Contaminated, non-interactive** → **denied**. This is the exact write ADR-078
  measured being *allowed* under substring taint.
- **Contaminated + `--accept-edits`** → one prompt with the provenance warning;
  approved write lands.

## Two wording bugs only the live run caught

The denial reason and the approval preview both still said "tainted data" after
the refactor. Unit tests assert `contains(...)`, not whether the sentence still
makes sense — the second time this sprint series that a live case caught
something green tests could not.

## Honest scope

This gates *mutation on a contaminated run*. It does not identify injections and
is deliberately **coarser** than what it replaced — every mutation after research
is gated, including obviously fine ones. That is the price of a control that
cannot be worded around, and it is the trade the decision accepted.

## Next

Fleet re-calibration (nothing since sprints 25–26; the 3B now sits alongside the
7B, same family, so a run isolates size), A5's sandbox (Docker), C7/C8/B1.
