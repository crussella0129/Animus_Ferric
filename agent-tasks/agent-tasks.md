# Agent Tasks (Persistent Backlog)

> Sprint 21 (fleet agentic capability map) is **done** — `bench --models` runs the
> full L0–L6 loop per model + a measured_level leaderboard. **Map: qwen2.5-coder:7b
> → 6 (Large); llama3.1:8b → 5 (Medium); llama3.2:1b → none (fails L0).** Key finding:
> a 1B fires single tool calls at 100% (toolbench) but can't *complete* a multi-turn
> task — single-shot reliability ≠ agentic capability; and the code-tuned 7B beats the
> larger general 8B. T-2101/2102 committed, ADR-030 amended.

Open candidates (sprint 22+):
- **Harder bench levels (L7+)** — to rank models *above* a 7B (qwen maxes L0–L6); a nice-to-have, not urgent (the ladder still discriminates 6/5/none).
- **More Ring-2 tools** (apply_patch).
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
- **Investigate the 1B's multi-turn failure** — does a smaller step budget / planner help it complete L0?
