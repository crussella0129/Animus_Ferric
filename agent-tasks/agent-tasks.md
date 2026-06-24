# Agent Tasks (Persistent Backlog)

> Sprint 14 (formalize the tool **rings**, [[ferric-tool-rings]]) is **done** — `ring`
> field + `ring_for_tier` + trim-from-outer `tools_for_policy` (alphabetical-cap bug
> fixed); Nano now gets exactly the 6-tool core, Small gets all 8; re-bench still 100%
> on both models. T-1401/1402 committed (ADR-028).

Open candidates (sprint 15+):
- **`--max-ring` CLI override** — pin a model to "exactly these rings" independent of tier (the user's "control exactly what rings").
- **Wire per-ring toolbench fire-rate → measured ring promotion** (the s13 100% is the `solid` bar).
- More Ring-1/2 tools (the find/organize + plan/diff rings).
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
