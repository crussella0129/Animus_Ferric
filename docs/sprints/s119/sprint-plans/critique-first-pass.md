# Plan Critique — Sprint 119 (first pass)

## Concerns

### C-001: Linux scope limitation exists only in sprint prose
- **Where:** build-plan T-11901/T-11902 Notes; INT-0008 AC-6 and Consequences.
- **Quote:** "General arbitrary group escape/owner-SIGKILL containment remains a documented backlog item."
- **Failure mode:** intent-drift
- **Why it matters:** The cooperative Unix scope and required surviving supervisor/reaper must be durable intent boundaries, not only sprint prose.
- **Suggested response:** fix-in-plan; record the partial increment without weakening eventual AC-6.

### C-002: Registry race clause needs a direct regression mapping
- **Where:** E06 and T-11902 unit tests.
- **Quote:** "serialize registry removal with signalling"
- **Failure mode:** plan-test-mismatch
- **Why it matters:** Late registration and stale signalling after normal removal are distinct interleavings.
- **Suggested response:** fix-in-plan; add a deterministic recorded-signal assertion for removal versus shutdown.

### C-003: Inherited capture handles are not explicitly exercised
- **Where:** E03 and T-11901 unit tests.
- **Quote:** "or descendant writers remain open"
- **Failure mode:** plan-test-mismatch
- **Why it matters:** Output volume alone does not exercise inherited-writer collection.
- **Suggested response:** fix-in-plan; require the controlled descendant to retain both stdout/stderr handles after its leader exits.

## Confidence
block

## Disposition
All three concerns were incorporated into the unlocked proposal and linked
intent before repeat review. This first-pass evidence is retained, not erased.
