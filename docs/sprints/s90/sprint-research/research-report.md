# Sprint 90 — Research Report

## 1. The decision

The user chose **Option 4, approval form**: gate on run-level provenance rather
than per-argument content, and ask a human rather than refusing outright.

## 2. Why the detector could not be salvaged

ADR-078's live measurement is the whole argument. Substring taint:

- detects **copying**; the threat is **influence** (an injection wins by being
  obeyed, not quoted);
- has one tuning axis — segment length — and both ends fail (long misses lifted
  fragments, short denies every write);
- is defeated by **paraphrase at any length**, and paraphrase is guaranteed here:
  the quarantine's summary already rewords the source, and a model restating it
  rewords again.

So no constant works. Re-tuning would have looked like progress and been none.

## 3. Why the structural form fits this codebase specifically

The quarantine is already structural — ADR-010/040 make empty-tools the only
valid constrained shape, so an injection has no action channel *by
construction*. The sink gate was the single place that reached for detection
instead, and it was the single place that did not hold up. Option 4 removes that
inconsistency rather than adding a mechanism.

## 4. What keeps it usable

Two properties, both tested:

- **A clean run is never gated.** Ordinary work — the overwhelming majority — is
  untouched. Without this the control would be turned off, which is the failure
  mode the previous design actually had.
- **One prompt per mutation**, inherited from ADR-079's merged gate, and denial
  when nobody can answer. Safe unattended, usable supervised.

## 5. What was deleted, and why that is the right call

`TaintSet` is gone rather than deprecated. A security control that does not fire
is worse than none: it manufactures confidence. Keeping it "for advisory
purposes" would have preserved exactly the impression the measurement disproved.

## 6. Honest limits

This gates *mutation on a contaminated run*. It does not identify injections,
cannot say which call is dangerous, and is deliberately **coarser** than what it
replaced — every mutation after research is gated, including obviously fine ones.
That coarseness is the price of a control that cannot be worded around.
