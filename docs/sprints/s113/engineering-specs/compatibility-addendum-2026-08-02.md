# Sprint 113 Contract Addendum — Compatibility Claim Boundary

Recorded 2026-08-02 after implementation preflight. This addendum clarifies the
approved build contract without weakening any controller, trace, or real-model gate.

## Workflow separation

These files are passive Ferric engineering notes. They are not Antigravity
artifacts and are not synchronized to any IDE task, approval, comment, or
walkthrough protocol. The Antigravity-native artifact names are deliberately
not used for Sprint 113. Progress is maintained through the repository's
tracked `agent-tasks/` ledger, ordinary Git commits, and test evidence.

## Old-binary fail-closed scope

The build contract's phrase “make old binaries fail closed on evidence-only events”
is too broad for trace schema v1:

- an older `replay()`/resume path rejects unknown events and therefore fails
  closed on an evidence trace;
- an older `trace verify` intentionally skips and counts unknown event types for
  additive schema tolerance, so it cannot validate the new controller causality
  but also cannot be claimed to reject every such trace.

The implemented guarantee is therefore:

1. old binaries cannot resume/replay an evidence trace as legacy state;
2. the Sprint 113 binary recognizes every controller event and validates its
   causal structure through the shared `TraceStructure` state machine;
3. real-model evidence is retained and verified only with the candidate binary
   whose hash is recorded in the run provenance;
4. no result treats an older verifier's unknown-event tolerance as evidence of
   controller validity.

A blanket every-consumer fail-closed guarantee would require a breaking trace
schema bump. That is unnecessary for the causal experiment and would damage the
approved backward-readability objective.

## Evidence-only screen execution seam

Implementation review found that the pre-Sprint-113 autonomy command only
expressed a legacy single-binary run or a two-arm paired run. That is
insufficient for the frozen development gate, which must run the evidence arm
alone on H01/H04/H08 before the candidate is frozen for paired confirmation.

The candidate therefore gains an explicit single-run `--harness-policy`
selector with these constraints:

1. omission remains the literal legacy behavior and sends no new child flag;
2. `evidence` labels the result as a single evidence coordinate and sends only
   `--harness-policy evidence` to the selected candidate binary;
3. `evidence-planner` remains rejected until its separately frozen design;
4. a single evidence screen must pass the same managed llama-server, exact
   model hash, context, non-negative seed, and one-slot provenance checks as a
   paired run;
5. paired control/candidate scheduling and the frozen control argv remain
   unchanged.

This adds the missing execution seam without weakening or changing the real
model gate. Mock and offline runs may test the seam, but cannot satisfy it.
