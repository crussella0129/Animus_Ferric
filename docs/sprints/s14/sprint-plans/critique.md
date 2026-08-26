# Plan Critique — Sprint 14

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Replacing `min_tier` is a 10-file edit
- **Failure mode:** blast-radius
- **Response:** **mechanical + compiler-checked.** It's a struct field rename with a per-tool value; the compiler enumerates every site (`spec.rs`, 8 builtins, the `registry.rs` dummy). No logic changes except the intended `tools_for_policy` trim. Low risk.

### C-002: Nano models lose `search_files`/`move_path` (now Ring 1)
- **Failure mode:** capability-regression
- **Response:** **intended — it's the whole point.** The rings model gives the smallest models the smallest, surest grammar; `search`/`move` unlock at Small+ where the model can carry more. And it's strictly *better* than today, where the Nano cap silently drops `write_file` (an essential) alphabetically. If the user wants them in Ring 0, it's a one-line change per tool — flagged in the research.

### C-003: ring is `u8` not an enum
- **Failure mode:** primitive-obsession
- **Response:** **accept (deliberate).** A `u8` ring index is open-ended (rings 2/3+ land later without an enum churn) and orders naturally for the trim. The semantics live in `ring_for_tier` + the per-tool assignment + the ADR, not the type.

### C-004: `ring_for_tier` reserves rings 2–3 with no tools yet
- **Failure mode:** dead-mapping
- **Response:** **accept.** It's the forward shape (planner=Ring 2, MCP=Ring 3); harmless today (no tools at those rings ⇒ those tiers just get rings 0–1). Documented in the ADR.

## Confidence
`clean` — a bounded, compiler-checked refactor that fixes a real cap bug and delivers the rings north star; reliability re-confirmed by the toolbench, not assumed.
