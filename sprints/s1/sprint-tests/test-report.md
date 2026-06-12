# Sprint 1 Test Report

## Summary
- Unit tests: 31 passed / 0 failed / 31 total (default-feature unit sets across core/trace/provider/guard/tools/cli, incl. 6 carried-s0 suites updated for CheckRecords; plus 11 feature-gated provider tests run locally)
- Integration tests: 24 passed / 0 failed / 24 total (ferric-loop 18 across 4 binaries — incl. critique-added `unknown_tool_feeds_back`; ferric-cli 6)
- E2E tests: 1 passed / 0 failed / 1 total — **the L0 smoke, Ferric's first real-model gate, PASSED twice** (original + re-run after critique C-001 tightening: causally-ordered allow check → write_file result)
- CI status: green — run 27316272754 conclusion=success on head `fa3bc04`, all three jobs (fmt/clippy/test win+linux, aarch64 check, NEW backend-check cold-compiling mistralrs under -D warnings), verified via `gh run list` as a separate step. The test-phase close commit (370cda7) re-runs the same gate.

## Failures
None in final state. Two real findings were diagnosed and fixed en route:
1. **Debug-profile inference unusable (~1 tok/s):** the first smoke attempt spawned the debug binary; a single turn ran 37+ min before being killed. Root cause (candle CPU kernels unoptimized in debug), not symptom, addressed: `--release` mandated in the smoke docs. The flush-per-event trace design proved its worth — the live trace showed the run mid-turn-0, distinguishing "slow" from "hung".
2. **fmt drift on l0_smoke.rs** (committed before a fmt pass) — fixed in a follow-up commit.

## Measured Actuals (closes research §4 unknowns)
- Llama-3.2-1B Q4_K_M on CPU (release): model load + 3-turn task = **116.9–126.7 s wall**, 223 output tokens, ~2.08 GB RSS.
- Observed 1B behavior (lineage-consistent): executes real tool calls correctly but *described* `task_complete` in prose on its final turn (clean `final_text` ending). This is the exact target of the s2 unified action grammar.

## Technical Debt Identified
- **No model-free test of MistralRsProvider::complete()'s validate-first line** (critique C-002, deferred): structurally impossible without a test-only abstraction; s2's HTTP backend will extract a shared `validated_complete` wrapper that is model-free testable.
- **Per-turn output-token budget missing from RunPolicy** (s2 backlog): turns are capped, generation length is not.
- **L0 gate does not prove the terminator protocol on a real model** (critique C-004, intentional): terminator mechanics are mock-proven; real-model protocol quality is the L1+/unified-grammar ladder.
- mistralrs 0.8.1 lacks `strict` tool-argument grammars (master has it) — strict mode returns with a dep bump.

## Coverage Observations
- Every EARS clause in the locked build-plan maps to a test or a recorded structural justification (unit-tests.md table); the critic confirmed no unjustified gaps.
- The loop is the best-covered subsystem (18 integration tests over golden trace order, terminator, repetition, backoff, budgets, denial/unknown feedback, ADR-010 shape).
- ADR-009 is now operational, not aspirational: the real-GGUF gate ran twice this sprint and caught a real environment-class issue on its first outing.
