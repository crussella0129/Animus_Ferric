# Sprint 15 Meta

- **Sprint number:** 15
- **Start timestamp:** 2026-06-24T21:50:14Z
- **End timestamp:** 2026-06-24T23:15:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Shipped `--max-ring` — the explicit "control exactly which rings" lever. `RunPolicy.max_ring` + restrict-only `min(tier_ceiling, override)` in `tools_for_policy`; `--max-ring` on query/toolbench. `--max-ring 0` pins any model to the Ring-0 core grammar regardless of size; expansion stays earned via `measured_level`. Proven end-to-end via the trace's offered_tools. ADR-028 amended.
