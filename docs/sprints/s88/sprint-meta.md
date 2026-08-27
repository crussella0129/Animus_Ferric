# Sprint 88 Meta

- **Sprint number:** 88
- **Start timestamp:** 2026-07-25T05:19:09Z
- **End timestamp:** 2026-07-25T06:55:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Fix sprint 87's G1, download a second (3B) model, and use it to
  measure the taint path live.

## Outcome

518 → **524 tests, 0 failures**; clippy 0; fmt clean.

**G1 root cause was sharper than expected.** Not retrieval failing in general —
the query was matched as **one literal lowercase substring**, so
`"configuration"` matched a file while `"project notes configuration"` matched
nothing in the same directory. Same defect class as A6, different component.
Queries are tokenized now, terms ANDed. And the silence that hid it for three
sprints (an empty research result skipped quietly) now reports itself.

**The most valuable result contradicts our own earlier finding.** With G1 fixed,
E2 could finally be measured against a real digest — and the write was
**allowed**, not blocked. ADR-075's synthetic "3/3 benign writes blocked" fed the
source's own sentences back as whole segments; reality inserts two mismatches:
the digest is a **paraphrase**, and the model writes a **prefix** of a sentence
rather than the whole thing. `contains()` fails on both.

**The honest reading: the mechanism is weak in both directions**, not
over-strict. The same two mismatches that spare a benign write would spare a
paraphrased injection. The "unusable in practice" framing is retired.

## Method note

The synthetic probe was right to exist — it isolated the mechanism — and wrong to
be trusted as a *rate*. **A probe that constructs its own worst case measures the
worst case, not the field.** Both numbers stay in the record.

## Model

`Qwen2.5-Coder-3B-Instruct-Q4_K_M` (1.93 GB, ungated) downloaded into `models/`
rather than mounted — the ZimaBoard2 share was unreachable. Same family as the
7B, so size is the only variable for a future calibration.

## Still open

The E2 posture decision (now better framed), fleet re-calibration (nothing since
sprints 25–26), A5's sandbox (Docker absent).
