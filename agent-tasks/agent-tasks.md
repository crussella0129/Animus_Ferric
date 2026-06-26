# Agent Tasks (Persistent Backlog)

> Sprint 22 (why the 1B isn't an agent) is **done** — diagnosed the 1B's L0 failure
> (repeat-not-terminate + semantic flailing) and tested a sharper repetition nudge.
> **It didn't move the 1B** (still `measured_level: none`), so the ceiling is a real
> capability limit, not wording (ADR-031). The nudge ships anyway. First sprint under
> one-PR-per-sprint ([[one-pr-per-sprint]]).

Open candidates (sprint 23+):
- **No-progress / semantic-flailing guard** — the repetition guard misses same-tool/different-args loops (L2: 15 `make_dir`s → `max_turns`). A "max consecutive same-tool" or no-workspace-change cap would catch it.
- **Harder bench levels (L7+)** — to rank models above a 7B.
- **More Ring-2 tools** (apply_patch).
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
