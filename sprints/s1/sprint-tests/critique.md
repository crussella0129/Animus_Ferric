# Test Critique — Sprint 1

> Critic: subagent (adversarial review per prompts/test-critic.md), 2026-06-11.
> Primary-agent dispositions inline as **Response:**.

## Concerns

### C-001: E2E assertion #6 lacks causal correlation
- **Where:** l0_smoke.rs assertion 6 — permission_check matched anywhere, not tied to the write_file path or ordered before its result.
- **Failure mode:** integration-drift
- **Response: TIGHTENED.** The smoke now locates the allow permission_check whose `path` contains hello.txt and asserts its trace index precedes the successful write_file tool_result (guard-before-handler proven causally). Re-run against the real model after the change.

### C-002: No model-free unit test of MistralRsProvider::complete() rejecting both-set requests
- **Failure mode:** EARS-coverage
- **Response: DEFER-WITH-RATIONALE.** Structurally impossible without inventing an abstraction purely for the test: `MistralRsProvider` cannot be constructed without a loaded engine (`mistralrs::Model` field). The guarantee is layered and each layer IS tested: the loop never builds the shape (`adr010_request_shape`), `validate()` itself is matrix-tested, and `complete()`'s first line calls it (documented as the ADR-010 boundary in the module docs). Recorded follow-on for s2: when the HTTP backend lands, extract a shared `validated_complete` wrapper both backends use, which is model-free testable — added to the s2 backlog item for the HTTP backend.

### C-003: No integration test for unknown tool calls
- **Failure mode:** negative-path
- **Response: ADDED.** `unknown_tool_feeds_back` in loop_core.rs: hallucinated tool name → "unknown tool: frobnicate" fed back as an error tool result, loop continues to recovery. Passing.

### C-004: L0 smoke passes with degraded terminator behavior (final_text instead of task_complete)
- **Failure mode:** e2e-drift (intentional)
- **Response: DEFER-WITH-RATIONALE (per critic's own analysis).** L0 gates capability (correct file op + clean termination), not protocol quality. The 1B describing task_complete in prose is the documented lineage failure family; making small models actually *call* the terminator is precisely the s2 unified-action-grammar work (ADR-010 note + backlog). The terminator mechanism itself is fully covered model-free (5 terminator tests + the mock query e2e ends with reason task_complete).

## Confidence

`proceed-with-caveats` (critic) → C-001 tightened (and re-validated against a real run), C-003 added and passing, C-002/C-004 deferred with recorded rationale and s2 follow-ons. Finalizing test-report.md after CI conclusion + smoke re-run.
