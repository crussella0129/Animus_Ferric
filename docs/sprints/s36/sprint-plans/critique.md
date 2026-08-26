# Plan Critique — Sprint 36

(Critic: foreground `Agent` tool, adversarial review against `prompts/plan-critic.md`.)

## Concerns

### C-001: T-3601's "reusable provider" claim glosses over the tokio Runtime, which is not reused
- **Failure mode:** hidden-dep
- **Response:** **fix-in-plan.** Added a note to T-3601 and rewrote T-3606's body/EARS to state
  explicitly that `run_mcp` builds ONE `tokio::runtime::Runtime` at launch and calls
  `runtime.block_on(...)` once per subsequent `tools/call` on that same instance (standard,
  supported tokio usage) — not just the provider. See `build-plan.md` T-3601 Notes + T-3606 body.

### C-002: T-3602's "reusable run config" elides that profile read-back staleness is a real behavior divergence from `ferric query`, stated as if it were free
- **Failure mode:** hidden-dep / EARS-vague
- **Response:** **fix-in-plan.** Added an explicit "Accepted tradeoff" paragraph to T-3602 stating
  the profile (`measured_level`/`calibrated_ring`, ADR-029) is read once at MCP launch and held for
  the process lifetime — a concurrent `ferric bench --calibrate-rings` run will not be picked up
  until restart — and that this is deliberate, matching the launch-time-fixed philosophy already
  applied to workspace/backend/model. T-3607 (ADR-046) is required to record this explicitly, not
  leave it implicit.

### C-003: T-3605's "without crashing the process" EARS clause has no test proving the server keeps serving after an error
- **Failure mode:** plan-test-mismatch
- **Response:** **fix-in-plan.** Reworded T-3605's EARS clause to state the server "SHALL
  continue accepting and correctly serving subsequent `tools/call` requests" after an error, and
  added a new integration test `error_then_success_same_session` (test-plan.md) driving two calls
  through the same dispatch session — one erroring, one succeeding — over the same pipe/session.

### C-004: The test-plan's "possible via --mock" E2E claim depends on a `--mock` flag that T-3606 never explicitly required
- **Failure mode:** plan-test-mismatch
- **Response:** **fix-in-plan.** T-3606 now explicitly states `McpArgs` includes `mock: bool` and
  adds an EARS clause: "WHEN `--mock` is passed, THEN `run_mcp` SHALL construct a `MockProvider`
  ... instead of calling `create_provider`."

### C-005: T-3607's EARS clause is an unmeasurable prose-content claim, and no test-plan section names it at all
- **Failure mode:** EARS-vague / plan-test-mismatch
- **Response:** **defer-with-rationale**, per the critic's own suggestion. Docs/ADR content isn't
  unit-tested elsewhere in this project either (sprint 35's ADR-045 had no automated test). Added
  an explicit "## Docs (T-3607)" section to test-plan.md stating this is a manual-review gate
  against the EARS clause, so the omission reads as a decision, not a gap.

### C-006: ADR-014 (the original roadmap ADR that first placed `ferric mcp` on the backlog) isn't cited in the research report's Decisions Reviewed
- **Failure mode:** ignored-ADR
- **Response:** **reject**, matching the critic's own assessment. ADR-014 is a sequencing/roadmap
  ADR; its `ferric mcp` line item is fully superseded by ADR-012's substantive design (which IS
  cited), and its Docker/Nix half is out of scope here and already tracked under Ornstein's own ADR
  chain (ADR-041 onward). Adding the citation would be paperwork with no effect on plan content.

### C-007: T-3605's file-routing reuse claim risks silently duplicating `query.rs`'s inline orchestration rather than truly reusing it
- **Failure mode:** granularity / hidden-dep
- **Response:** **fix-in-plan**, option (a) from the critic's suggestion. T-3605 now touches
  `query.rs` as well as `mcp.rs`: it extracts the per-file routing loop (today inline in
  `run_query`, around the `classify_path`/`decide_attachment` calls) into a shared function called
  by both `run_query` and the new `tools/call` handler. Test-plan's file-routing test now states
  plainly that new fixtures are authored here (query.rs has no pre-existing `#[test]` module to
  reuse — confirmed by inspection), dropping the earlier "if any" hedge.

## Confidence

**proceed-with-caveats → all caveats addressed above (6 fix-in-plan, 1 defer-with-rationale, 1
reject with stated reason).** Plans are ready to lock.
