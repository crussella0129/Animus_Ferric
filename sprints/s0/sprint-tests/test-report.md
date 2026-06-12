# Sprint 0 Test Report

## Summary
- Unit tests: 33 passed / 0 failed / 33 total (ferric-core 8, ferric-trace 4, ferric-provider 3, ferric-guard 9, ferric-tools 4, ferric-cli 3, builtin file tools 4 — exact names mapped to EARS clauses in unit-tests.md; symlink-escape test runs on the ubuntu CI leg)
- Integration tests: 3 passed / 0 failed / 3 total (`guarded_traced_execution`, `mock_loop_skeleton`, `tier_table_snapshot`)
- E2E tests: N/A — not yet possible by design; unlocked by s1 L0 smoke (see e2e-tests.md)
- CI status: green — run on head `aeb7554` conclusion=success (fmt/clippy/test on windows+ubuntu, aarch64-unknown-linux-gnu check), verified via `gh run list` as a separate step. The post-critique tightening commit re-runs the same gate.

## Failures
None. One mid-build clippy finding (type_complexity in the snapshot test) was fixed at the cause (tuple → named struct) before its task commit; one trait-derive error (Deserialize on `&'static str` deny reasons) fixed by making decisions serialize-only, matching their actual data flow.

## Technical Debt Identified
- **Policy-driven constraint selection is not yet exercised** (critic C-002): `mock_loop_skeleton` hardcodes its JSON-Schema constraint rather than deriving it from `RunPolicy.protocol`. Lands with the s1 production loop + first real backend.
- **Tier table is a calibration seed, not calibrated truth** (ADR-006): pinned by snapshot, recalibrated when the L0–L6 benchmark harness is ported in s1.
- **Sync `Tool::run`**: converts to async at the registry chokepoint when s1's exec tool needs timeout/cancellation (single call site by design).
- Deferred lineage fixes tracked in `agent-tasks/agent-tasks.md` (repetition guard, structured terminator, backoff, bounded reads, stale-config migration, circuit-breaker compaction).

## Coverage Observations
- Every EARS clause in the locked build-plan has ≥1 named test (mapping table in unit-tests.md); critic confirmed no coverage gaps.
- Security paths are the deepest-covered area (boundary: 6 tests incl. prefix-collision and symlink escape; checker: 4) — consistent with the lineage's "≥80% on security paths" principle.
- The full untruncated-trace vs truncated-for-model split is asserted at three layers (trace unit, registry unit, capstone integration).
- aarch64-unknown-linux-gnu type-check passes (locally and in CI), holding the Pi/Orin portability floor with zero platform-specific code so far.
