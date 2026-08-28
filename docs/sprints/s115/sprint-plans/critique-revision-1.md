# Plan Critique — Sprint 115 (Revision 1)

## Concerns

### C-001: Black-box post-create test cannot access the specified test-only seam
- **Where:** `build-plan.md` T-11414 Notes; `test-plan.md` integration coverage
- **Quote:** “its symbol is absent from non-test builds” versus integration test-seam substitutions.
- **Failure mode:** hidden-dep
- **Why it matters:** A normally compiled integration-test binary cannot access a unit-only seam.
- **Response:** fixed — substitution coverage is unit-only; CLI integration retains feasible process-level assertions.

### C-002: INT-0008 AC-6 is claimed without its high-level execution subject
- **Where:** build/test plan intent traceability versus INT-0008 AC-6
- **Quote:** “acceptance criterion covered: AC-6.”
- **Failure mode:** intent-drift
- **Why it matters:** The sprint explicitly defers the high-level workflow named by AC-6.
- **Response:** fixed — AC-6 remains open; query/release results are enabling evidence only.

### C-003: Harness self-tests lack a postcondition restoring the clean-run boundary
- **Where:** T-11502 / E16-B and its verification
- **Quote:** self-tests begin with clean roots but had no final absence gate.
- **Failure mode:** plan-test-mismatch
- **Why it matters:** Self-tests may recreate exactly the generated state the canonical run requires absent.
- **Response:** fixed — regenerated roots are manifested and losslessly re-quarantined, then all four canonical roots are re-proved absent immediately before handoff.

### C-004: Cold-preflight completeness remains subjective
- **Where:** T-11503 / E17-A
- **Quote:** “relevant processes” and “observations are complete.”
- **Failure mode:** EARS-vague
- **Why it matters:** Required resource, process, listener, runfile, identity, and sandbox fields were not enumerated.
- **Response:** fixed — the clause and test now enumerate every required timestamp, memory, GPU, exact-image process, owned PID, endpoint/listener, local/global runfile, model, engine, WSL, Bubblewrap, and network-probe field.

## Confidence
block
