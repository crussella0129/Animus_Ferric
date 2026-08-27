# Sprint 82 Meta

- **Sprint number:** 82
- **Start timestamp:** 2026-07-24T19:41:19Z
- **End timestamp:** 2026-07-24T20:31:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Full-codebase verification of all 14 crates + the Dark Matter seam
  — ran the three checks sprint 81 was blocked on, and converted its
  inspection-derived findings into demonstrated ones.

## Outcome

Baseline green: 463 tests pass / 0 fail (52 suites), clippy 0 warnings, fmt
clean, cold rebuild 31s / 0 warnings, DM verifier PASS 61 / FAIL 0.

Four defects proven to live *inside* that green suite, each by a test written to
fail on `main`: A1 (20,028 chars reach the model where ADR-002 promises 4,000),
A3 (staged git index destroyed once per turn), A6 (`fetch_reference` finds
nothing for ≤2-char queries), plus the Dark Matter contract divergence (a
DM-schema-legal call is hard-rejected). A2/A4/A5/A7 confirmed by inspection.

Three corrections to sprint 81 — most importantly that **its own recommended fix
for A3 (`git read-tree HEAD`) destroys the index identically to `git reset`**; a
temporary `GIT_INDEX_FILE` is the verified fix.

B1–B8, C1–C8, D1–D3 all confirmed; B3 proven by removing the six unused deps and
compiling clean.

## Scope

Audit only, by design — the tree is unmodified apart from three documents
(`docs/verification-2026-07.md`, ADR-072 in `decisions.md`, README sprint-log
entry). All probe tests were run, recorded, and deleted; all Cargo experiments
reverted. Remediation of A1–A7 is ordered in the report's §8 and belongs to s83.

## Notable

The defects share one shape: **each is covered up to a crate boundary and not
across it.** That, not any individual bug, is the finding worth acting on.
