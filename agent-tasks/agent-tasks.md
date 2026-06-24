# Agent Tasks (Persistent Backlog)

> Sprint 10 (multimodal "any file" input) build phase is **complete** — T-1001..T-1004
> all committed (see `agent-tasks/completed-tasks.md`). Remaining: test phase
> (unit/integration green; live-media E2E is the DEFERRED heartbeat) → loop close + ADR-026.

Deferred (heartbeat): **live-media E2E** — a real multimodal model (Gemma 3n)
reading an attached image/audio clip end-to-end. Needs a multimodal-capable
server the dev machine lacks (route TBD: `llama-server`+mmproj vs an ollama
vision pull). The whole input pipeline is unit/integration-tested; only this
final "real model reads the bytes" check is deferred.
