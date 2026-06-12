# Test Critique — Sprint 0

> Critic: subagent (adversarial review per prompts/test-critic.md), 2026-06-10.
> Primary-agent dispositions recorded inline as **Response:**.

## Concerns

### C-001: T-003 nano_policy_shape missing prompt_budget assertion
- **Where:** `crates/ferric-core/src/scale.rs` test `nano_policy_shape` vs build-plan T-003 EARS.
- **Quote:** test asserted tier/protocol/planner/max_tools but not `prompt_budget_tokens`.
- **Failure mode:** weak-assertion
- **Why it matters:** the primary EARS test for NANO shape should pin every policy field it describes, not lean on the "extra" budget test.
- **Suggested response:** tighten-assertion.
- **Response: TIGHTENED.** Added `assert_eq!(policy.prompt_budget_tokens, 2_800)` and `assert!(!policy.allows_subagents)` to `nano_policy_shape`; full gate re-run green. (Build-plan is locked, so the fix is test-side only — the clause's intent is now fully pinned.)

### C-002: mock_loop_skeleton hardcodes the constraint instead of deriving it from policy
- **Where:** `crates/ferric-provider/tests/mock_loop_skeleton.rs`.
- **Quote:** `let constraint = Constraint::JsonSchema(json!({"type": "object"}));` ... `assert!(requests.iter().all(|r| r.constraint.is_some()));`
- **Failure mode:** weak-assertion
- **Why it matters:** the test proves constraint *plumbing*, not policy-driven constraint *selection*.
- **Suggested response:** defer-with-rationale.
- **Response: DEFERRED.** Policy→constraint derivation (per-tool JSON-Schema grammars selected by `RunPolicy.protocol`) is exactly the s1 production-loop work; this test is the acknowledged test-only template. Recorded as technical debt in test-report.md and covered by the existing s1 backlog item "Structured terminator wired into constraint grammar".

## Confidence

`proceed-with-caveats` (critic) → C-001 tightened and re-run green; C-002 deferred with rationale into the s1 backlog. Finalizing test-report.md.
