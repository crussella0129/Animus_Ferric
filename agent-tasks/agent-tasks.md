# Agent Tasks (Persistent Backlog)

> Sprint 10: multimodal "any file" input — text/code as text (any model),
> image/audio/video as capability-gated media parts on the OpenAI valve
> (ADR-023/025). Pipeline is pure + unit-tested; live-media E2E deferred to a
> heartbeat (no multimodal server on the machine). Plan: `sprints/s10/sprint-plans/build-plan.md`.

- [x] T-1001 (sprint 10): `Message` media parts + routing logic — DONE (e60d6d5)
- [x] T-1002 (sprint 10): OpenAI multimodal mapping + `Capabilities.supports_media` — DONE (4ab6944)
- [ ] T-1003 (sprint 10): `ferric query --file/--modality` wiring + gating — touches: crates/ferric-cli/src/query.rs + crates/ferric-loop/src/run.rs (RunArgs)
- [ ] T-1004 (sprint 10): Docs + README timeline — touches: README.md, docs/multimodal.md

### T-1003 implementation notes (for the next loop iteration)
- **Thread media into the loop:** add `pub media: Vec<MediaPart>` to `RunArgs` (crates/ferric-loop/src/run.rs); change the initial-message line (`run.rs:83`) from `Message::user(prompt)` to `Message::user_with_media(prompt, args.media.clone())` (empty Vec ⇒ identical to today). Update all 4 `RunArgs {}` sites: query.rs `drive_mock`(355) + `drive_real`(390) carry the real media; `backoff_tests.rs:75` + `common/mod.rs:176` get `media: Vec::new()`.
- **CLI:** add `--file <PathBuf>` (repeatable `Vec`) + `--modality <String>` to `QueryArgs`. In `run_query` before the mock/real split: `parse_modalities(&args.modality…)`; for each `--file` run `classify_path` + `decide_attachment(kind, &declared, caps.supports_media)` (caps already computed ~query.rs:132). AppendText → read to string, append to an `effective_prompt`; Media → read bytes → base64 → `MediaPart`; Skip → `eprintln!` the reason. Pass `effective_prompt` (instead of `&args.prompt`) + the `media` Vec into both `drive_mock`/`drive_real` (add params).
- **base64:** check `Cargo.lock` for the `base64` crate (likely transitive via reqwest); if present add as a direct dep of ferric-cli (cheap, ADR-004-ok), else a ~15-line inline encoder. Needed for `MediaPart.data`.
- **Tests:** `--mock` integration — `.md` file appends to the prompt; `.png` without `--modality` is Skipped with a surfaced reason; `.png --modality image` puts one `MediaPart` on the message.
- Then **T-1004** docs, then test phase (unit/integration green; live-media E2E is the DEFERRED heartbeat), then loop close + ADR-026.

Deferred (heartbeat): live-media E2E — Gemma 3n reads an image/clip via a
multimodal server (route TBD: llama-server+mmproj vs ollama vision pull).
