# Agent Tasks (Persistent Backlog)

> Sprint 14: formalize the tool **rings** ([[ferric-tool-rings]]). A `ring` per tool,
> capability-gated by `ring_for_tier`, and `tools_for_policy` **trims from the outer
> ring first** (fixing the alphabetical `max_tools` cap). Plan: `sprints/s14/sprint-plans/build-plan.md`.

- [ ] T-1401 (sprint 14): `ring` field on `ToolSpec` (replaces `min_tier`) + `ring_for_tier` + ring-aware trim-from-outer `tools_for_policy` + ring assignments + tests — touches: crates/ferric-tools/src/{spec.rs,registry.rs,builtin/*.rs}, crates/ferric-core/src/scale.rs
- [ ] T-1402 (sprint 14): ADR (ring architecture) + README/docs + re-bench — touches: decisions.md, README.md, docs/

Later: a `--max-ring` CLI override + explicit measured-promotion-from-toolbench
wiring; MCP-stdio (ADR-012); live-media heartbeat (human-gated).
