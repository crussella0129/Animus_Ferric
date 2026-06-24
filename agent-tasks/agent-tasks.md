# Agent Tasks (Persistent Backlog)

> Sprint 12 (the `search_files` tool) build phase is **complete** — T-1201 (tool +
> 6 tests) + T-1202 (docs) committed. The agent's builtin tool surface now has
> grep-style content search. Remaining: test phase (all green; no deferred E2E)
> → loop close.

Larger follow-on candidate: **MCP-stdio integration** (ADR-012) — connect Ferric
to external MCP tool servers. The **live-media E2E heartbeat** (sprint-10 deferral)
remains human-gated (needs a multimodal server). Revisit mistral.rs constraints
only when upstream fixes the llguidance-on-GGUF hang (ADR-027).
