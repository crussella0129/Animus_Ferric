# Agent Tasks (Persistent Backlog)

> Sprint 11 (mistral.rs constrained-decoding spike) is **complete** — T-1101 wired
> `set_constraint`, the bounded probe showed mistralrs 0.8.15 **still hangs** on a
> trivial GGUF schema (ADR-027), so the wiring was reverted (no regression).
> mistral.rs stays text-only; the HTTP valve remains the sole constrained path.

Open candidates for a future sprint:
- **Live-media E2E heartbeat** (sprint-10 deferral) — human-gated on standing up a multimodal server (`llama-server`+mmproj or an ollama vision pull).
- **MCP-stdio integration** (ADR-012) — connect Ferric to external MCP tool servers.
- Revisit mistral.rs constraints only when upstream fixes the llguidance-on-GGUF hang (ADR-027).
