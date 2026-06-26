# Agent Tasks (Persistent Backlog)

> Sprint 20 (full agentic loop on the real backend) is **done** — wired the openai
> backend into the L0–L6 bench runner (`bench --backend openai`), fixed the
> verification bug it surfaced (the `task_complete` terminator wasn't credited), and
> ran it: **qwen2.5-coder:7b passes ALL of L0–L6 on the constrained path →
> measured_level 6 (Small→Large)**; `query --profile-dir` reads it back. The
> multi-turn agentic loop is validated end-to-end, not just single tool calls.
> T-2001/2002 committed, ADR-030.

Open candidates (sprint 21+):
- **Harder bench levels** — qwen-7b maxes L0–L6; add L7+ (bigger multi-file projects) so the ladder discriminates capable models again.
- **More Ring-2 tools** (apply_patch) — Ring 2 is proven drivable.
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
