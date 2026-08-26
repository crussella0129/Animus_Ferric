# Sprint 7: Re-align Ferric to the Constrained-Decoding Thesis

**Status**: complete
**Start timestamp**: 2026-06-22T22:42:00Z
**End timestamp**: 2026-06-23T21:30:00Z
**Model**: Claude Opus 4.8
**Exit status:** success

## Objective
**Re-framed** (the original "cure toolbench" objective was symptom-level). Research
established the toolbench failures are symptoms of architectural drift away from the
founding thesis — *the harness owns decoding so a small model cannot emit a malformed
tool call*. Re-align the code to that thesis:
1. Reinstate the `Constraint` on `CompletionRequest` and re-enforce ADR-010 in `validate()`.
2. Make the OpenAI-compatible HTTP backend carry a harness-authored JSON-Schema
   (`response_format`) that the server enforces — the thesis on the backend that works
   (ADR-001 escape valve, out-of-process, pure Rust on our side).
3. Delete the PyO3/PyTorch backend (ADR-013 realignment; user-confirmed).
4. Make `capabilities()` honest; rebuild the toolbench to measure the real path.

User-confirmed direction this session: HTTP valve carries the constraint; delete Python.
Plan: `sprint-plans/build-plan.md` (T-001..T-008). Critique: `sprint-plans/critique.md`
(proceed-with-caveats, fixes applied). mistral.rs in-process *constrained* path stays
backlog (upstream llguidance hang, ADR-020).

## Phases
- [x] Phase 1: Initialize
- [x] Phase 2: Research
- [x] Phase 3: Plan
- [x] Phase 4: Build (T-001..T-008, 7 commits, all green)
- [x] Phase 5: Test (122/0 default; backend-openai + backend-mistralrs clippy clean; mock E2E green)
- [x] Phase 6: Loop

## Outcome
All 8 build tasks landed AI-verifiable-green on branch `sprint-7-realign` (not yet
pushed). The constrained-decoding thesis is restored on the HTTP valve and the
PyO3 backend is gone (−450 LOC net on that change). **Two human-gated steps remain
before merge:** (1) the real-model E2E acceptance / ADR-009 merge gate — the user's
visual heartbeat against a running server (see `sprint-tests/e2e-tests.md` E2E-1);
(2) the push/PR/merge decision (outward-facing). Confidence: 0.7 → 0.8 (pass).
