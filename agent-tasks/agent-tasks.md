# Agent Tasks (Persistent Backlog)

> Sprint 15 (`--max-ring` override) is **done** — `RunPolicy.max_ring` + restrict-only
> `min(tier_ceiling, override)` in `tools_for_policy`; `--max-ring` on query/toolbench.
> `--max-ring 0` pins any model to the Ring-0 core grammar. T-1501/1502 committed
> (ADR-028 amended). The rings are now fully controllable ([[ferric-tool-rings]]).

Open candidates (sprint 16+):
- **Measured ring promotion** — wire the per-ring toolbench fire-rate into `measured_level`/ring unlock (the s13 100% is the `solid` bar); the last piece of "rings expand as the model proves itself."
- More **Ring-1/2 tools** (find/organize, plan/diff).
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
