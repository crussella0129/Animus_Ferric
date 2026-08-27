# INT-0004 — Auditable session provenance

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0004
- **State:** proposed
- **Work evidence:** [T-11403/T-11404 backlog](../work/tasks.md#book-v2-carry-forward-from-sprint-113)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 113 gap audit](../sprints/s113/sprint-research/research-report.md)

## Intent

Make a Ferric session independently answer what prompt, policy guidance, tool
descriptions, action schema, sampling configuration, workspace effects, and
trace bytes governed each turn. Ordinary traces must detect tampering and be
easy to inspect without executing recorded tools or depending on one machine's
absolute paths.

## Acceptance criteria

1. Trace provenance binds canonical hashes for the effective prompt layers,
   Evidence guidance, tool descriptions, action schema/constraint, backend
   capabilities, and sampling configuration actually sent.
2. A versioned manifest or hash chain detects modified, reordered, inserted,
   removed, or cross-session records while distinguishing a valid recoverable
   crash prefix from corruption.
3. `trace verify` remains side-effect-free, supports Legacy traces explicitly,
   and reports which integrity or genealogy check failed without exposing
   secrets.
4. Audit commands provide deterministic session listing, per-turn workspace
   effect/diff views, and an explicit safe `resume-last` or fork selection
   without guessing workspace identity.
5. Portable provenance separates stable relative identity from local absolute
   paths; known-vector, migration, tamper, and concurrent-allocation tests pass.

## Rationale

Sprint 113 used manual Git-tracked hashes to protect one experiment, but normal
product traces have structural validation only. The analyses also identified
prompt/schema genealogy and session audit ergonomics as prerequisites for
debugging controller behavior without folklore.

## Alternatives

- Rely on Git or filesystem immutability: rejected because ordinary user traces
  are outside the sprint archive and may never be committed.
- Store only the final composed prompt: insufficient to attribute which policy
  or schema component changed.
- Add a GUI before the data contract: deferred; CLI-readable provenance is the
  first independently testable boundary.

## Consequences

Trace formats gain additive identity fields and integrity lifecycle rules.
Redaction and portability become first-class design work, and existing traces
must remain readable without being misrepresented as integrity-bound.

## Transition history

- 2026-08-26: created as `proposed` from the Sprint 113 provenance and audit gaps.
