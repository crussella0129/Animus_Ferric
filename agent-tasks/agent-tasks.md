# Agent Tasks (Persistent Backlog)

> Sprint 13 (complete Ring 0) is **done** — `edit_file` + `delete_path` shipped and
> the full 8-tool core measured at **100% fire rate on qwen2.5-coder:7b AND
> llama3.2:1b** (the "retain 100% reliability" gate). T-1301/1302/1303 committed.

**Next — Sprint 14: formalize the rings** ([[ferric-tool-rings]]):
- a `ring: u8` on `ToolSpec` (0 = core).
- ring-aware `tools_for_policy`: include rings `0..=active_ring`; **trim from the
  outer ring first** — fixing the latent alphabetical `max_tools` cap (already a
  real bug now that Nano has 8 tools > cap 6).
- a config ring-cap + measured auto-promotion (unlock ring N+1 when the toolbench
  scores `solid` on the rings inside it — the s13 100% baseline is the bar).
- an ADR for the ring architecture.

Larger later: MCP-stdio (ADR-012, needs the ADR-005 external-exec call); live-media
heartbeat (human-gated on a multimodal server).
