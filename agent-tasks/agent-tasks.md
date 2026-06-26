# Agent Tasks (Persistent Backlog)

> Sprint 19 (seed Ring 2) is **done** — added `multi_edit` (`ring: 2`), an ordered
> atomic batch of edits to one file, and `toolbench --params-b` so calibration can
> reach Ring 2. Live: qwen2.5-coder:7b at `--params-b 20` calibrates `--max-ring 2`
> — rings 0/1/2 (6/10/11 tools) all 100% solid. The 7B drives the nested-array
> `multi_edit` at 100% — Ring 2 is reachable. T-1901/1902 committed, ADR-028 amended.
> [[ferric-tool-rings]]

Open candidates (sprint 20+):
- **More Ring-2 tools** (apply_patch / structured-plan) — Ring 2 is proven drivable; widen it.
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
- **A 13B+ model** to calibrate Ring 2 by *tier* (not just via `--params-b`).
