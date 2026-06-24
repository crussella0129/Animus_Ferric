# Agent Tasks (Persistent Backlog)

> Sprint 10: multimodal "any file" input — text/code as text (any model),
> image/audio/video as capability-gated media parts on the OpenAI valve
> (ADR-023/025). Pipeline is pure + unit-tested; live-media E2E deferred to a
> heartbeat (no multimodal server on the machine). Plan: `sprints/s10/sprint-plans/build-plan.md`.

- [ ] T-1001 (sprint 10): `Message` media parts + `classify_path`/`decide_attachment` (pure) — touches: crates/ferric-core/src/message.rs, lib.rs
- [ ] T-1002 (sprint 10): OpenAI multimodal content mapping + `Capabilities.supports_media` — touches: crates/ferric-provider/src/{openai.rs,types.rs,mistralrs.rs}
- [ ] T-1003 (sprint 10): `ferric query --file/--modality` wiring + gating — touches: crates/ferric-cli/src/query.rs
- [ ] T-1004 (sprint 10): Docs + README timeline — touches: README.md, docs/multimodal.md

Deferred (heartbeat): live-media E2E — Gemma 3n reads an image/clip via a
multimodal server (route TBD: llama-server+mmproj vs ollama vision pull).
