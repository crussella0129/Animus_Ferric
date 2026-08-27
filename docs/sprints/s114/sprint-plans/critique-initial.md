# Plan Critique — Sprint 114 (initial)

## Concerns

### C-001: Quant fallback can select a coordinate declared non-viable
- **Where:** `build-plan.md` T-11409 E09-D / `model-selection.md` Frozen fallback rule / `test-plan.md` Managed model integration
- **Quote:** “select Q3 only if it completes and either Q4 did not …; otherwise Q4 remains selected if functional”
- **Failure mode:** plan-test-mismatch
- **Why it matters:** Research defined viability as functional operation at at least 2.0 decoded tokens/s, but E09-D could select a sub-2.0 Q3 whenever Q4 did not complete, or retain a functional sub-2.0 Q4 when Q3 failed the 25% improvement rule. The test plan stated a different rule.
- **Suggested response:** fix-in-plan

### C-002: Ring-2 app execution lacks a precommitted host-shell denial boundary
- **Where:** `build-plan.md` T-11410 E10-A / `sprint-loops-capability-audit.md` Capability layers / `INT-0007` Intent
- **Quote:** “Legacy policy … Ring 2 ceiling”; “Ring-2 `shell_exec` … is an unsandboxed host shell”
- **Failure mode:** missing-risk
- **Why it matters:** Bubblewrap contains grader execution, not Ferric’s own tool requests. The plan did not state how shell, Git, network, or out-of-workspace requests would be denied. Subsequent source reconciliation also found that the premise was wrong: query never registers the human-only shell tool.
- **Suggested response:** fix-in-plan

### C-003: The required continuation boundary remains conditional
- **Where:** `INT-0007` AC-3 / `build-plan.md` T-11410 E10-B / `test-plan.md` `mh_rs01_resume_lineage`
- **Quote:** “at least one continuation boundary”; “otherwise persistence is explicitly `not-observed`”
- **Failure mode:** intent-drift
- **Why it matters:** Early completion could avoid the continuation observation even though AC-3 requires one.
- **Suggested response:** fix-in-plan

### C-004: The no-repair assertion has no writer-attribution method
- **Where:** `build-plan.md` T-11410 E10-A / `test-plan.md` `mh_rs01_invocation_and_mutation_audit`
- **Quote:** “external file audit proves no Codex candidate edit”
- **Failure mode:** EARS-vague
- **Why it matters:** A before/after audit proves state changes, not which process authored them. Every mutation needs reconciliation to Ferric effects and unexplained writes need an invalidating rule.
- **Suggested response:** fix-in-plan

### C-005: README cleanup is narrower than the durable intent
- **Where:** `INT-0007` AC-7 / `build-plan.md` T-11413 E13-A / `test-plan.md` README cleanup tests
- **Quote:** “no longer carries sprint-specific result history”; “remove the Sprint-113-specific historical status/result paragraph”
- **Failure mode:** intent-drift
- **Why it matters:** A one-paragraph assertion did not prove the entire README was free of numeric sprint-result history.
- **Suggested response:** fix-in-plan

### C-006: The skill audit is hard-coupled to a successful Qwen3.8 coordinate
- **Where:** `build-plan.md` T-11411 dependencies / `research-report.md` Recommended Approach / `INT-0007` AC-5
- **Quote:** “Depends on: T-11409”
- **Failure mode:** hidden-dep
- **Why it matters:** If neither Qwen3.8 quant formed a usable endpoint, AC-5 could remain untested despite an existing attested model.
- **Suggested response:** fix-in-plan

### C-007: Model provenance tests do not verify all exact AC-1 fields
- **Where:** `build-plan.md` T-11407 E07-A/E07-C / `test-plan.md` Intent Traceability and T-11407 tests
- **Quote:** “revision `313447f...ed81` … SHA-256 `322e194f...3482`”; “exact size/hash verification against the publisher value”
- **Failure mode:** plan-test-mismatch
- **Why it matters:** Abbreviated plan values and partial test assertions could allow an incorrect provenance record to pass.
- **Suggested response:** fix-in-plan

### C-008: Runtime viability has no elapsed-time budget
- **Where:** `build-plan.md` T-11409 E09-A/E09-D and T-11410 E10-B / `test-plan.md` Managed model integration
- **Quote:** “4,096-token budget”; “2.0 decoded tokens/second”; “total budget capped at 28 turns”
- **Failure mode:** missing-risk
- **Why it matters:** The worst case was many hours without a fixed request/session cap, and one short smoke was not a reproducible throughput comparison.
- **Suggested response:** fix-in-plan

## Confidence
block
