# Sprint 88 — Research Report

## 1. Goal

Fix sprint 87's G1, and get a second, weaker model so the remaining live
validations become possible.

## 2. Model acquisition

The ZimaBoard2 GGUF library was unreachable (no sshd, `net view` RPC failure over
tailscale, no mapped drive, no guessable share name), so the user directed a
download instead. Chose **Qwen2.5-Coder-3B-Instruct-Q4_K_M** (1.93 GB, ungated,
`bartowski/…-GGUF`) over a 4B from another family: holding the family constant
against the existing 7B makes size the only variable, which is what a fleet
calibration needs.

## 3. G1's root cause was sharper than expected

Not a retrieval failure in general — `"configuration"` matched fine. The query
was matched as **one literal lowercase substring**, so `"project notes
configuration"` matched nothing in a directory containing all three words. Same
defect class as A6 (`fetch_reference`'s tokenizer), in a different component.

The silence (G1b) is what let it survive: `research_all` returns `Ok` with an
empty list, and the CLI skipped quietly. Both halves are fixed.

## 4. The most valuable result contradicts our own earlier finding

With G1 fixed, E2 could finally be measured against a real digest — and the
answer disagrees with ADR-075's synthetic 3/3-blocked. The write was **allowed**.

The synthetic probe fed the model's *own source sentences* back as whole
segments. Reality inserts two mismatches: the digest is a paraphrase, and the
model writes prefixes/extracts rather than whole segments. `contains()` fails on
both.

**This makes the mechanism weaker, not safer.** The same two mismatches that
spare a benign write would spare a paraphrased injection.

## 5. Conclusion

E2 remains a posture decision, but the decision is now differently framed: not
"the gate is too strict to use" but "sentence-granularity substring taint is
fragile in both directions". Re-tuning the constant is explicitly not a fix.
