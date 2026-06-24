# Sprint 10 Research Report — Multimodal "any file" input

> Pre-committed by ADR-023 and re-affirmed by ADR-025 as the sprint-10 feature:
> let Ferric take **any file** as input — text/code as text (any model), and
> audio/video/image as media parts for models that can read them (Gemma 3n). The
> foundation (constrained tool-calling, validated to 1B) is solid; this builds
> *up*. Key constraint from the sprint-8/9 findings: a **live-media E2E needs a
> multimodal-capable server the dev machine lacks** (no `llama-server`; ollama has
> only text models pulled) — so this sprint ships a **fully unit-tested input
> pipeline** and **defers the live-media heartbeat** to when such a server exists.

## Decisions Reviewed
- **ADR-023** — the multimodal design sketch: `Message` carries *content parts* (text OR media `{bytes/base64, mime}`); the OpenAI backend maps media to the OpenAI content array (`image_url` data-URL, `input_audio`); a capability-gated media-routing layer; functionality outranks Rust purity. **The blueprint.**
- **ADR-025** — sprint 9: constraint holds to 1B; the valve is the workhorse; mistral.rs stays text-only. So multimodal rides the **OpenAI valve only**; the mistral.rs path stays text.
- **ADR-006** — `ModelProfile` is **config-supplied, never inferred**. So *which* modalities a model accepts is declared config, not sniffed — modality gating keys off `ModelProfile`, consistent with the H8/H20 anti-misdetection stance.
- **ADR-003 / ADR-022** — `Message` is the shared type to extend; capabilities must stay **honest** (the trichotomy precedent) — a text-only backend (mistral.rs) must report it *cannot* carry media.

## Existing Code Survey
| File | Relevance | Notes |
|------|-----------|-------|
| `crates/ferric-core/src/message.rs` | high | `Message { role, text: Option<String>, tool_calls, tool_call_id }` — text-only. The additive change: a `media: Vec<MediaPart>` field (serde default-empty → backward-compatible; no churn on existing `msg.text` readers). |
| `crates/ferric-provider/src/openai.rs` (`map_message`) | high | Today `out["content"] = json!(text)` (a string). Multimodal: when `media` non-empty, emit the **content array** `[{type:text,…}, {type:image_url,image_url:{url:"data:…;base64,…"}}, {type:input_audio,…}]`; plain string otherwise. Pure → unit-testable. |
| `crates/ferric-provider/src/types.rs` (`Capabilities`) | high | `{ supports_native_tool_calls, supports_constraint, exposes_logits }`. Add an honest media flag (e.g. `supports_media`): the valve forwards media (true); mistral.rs cannot (false). |
| `crates/ferric-core/src/scale.rs` (`ModelProfile`) | high | Add declared `modalities` (image/audio/video) — config, ADR-006. The gating source of truth for *what this model accepts*. |
| `crates/ferric-cli/src/query.rs` (`QueryArgs`) | high | `prompt: String` only. Add `--file <path>` (repeatable). A router classifies each: text/code → read + append to the prompt (works on **any** model); media → a `MediaPart`, gated on `ModelProfile.modalities` ∧ `Capabilities.supports_media`. |
| `crates/ferric-tools/src/builtin/read_file.rs` | med | The text-file path already exists; the router reuses "read as text" for text/code files (the universal "any file" baseline). |
| `crates/ferric-trace` | med | Media bytes must NOT bloat traces — log a media part as `{mime, bytes_len, sha8}`, never the base64 blob. |

## External Sources
- **OpenAI multimodal content** — user content as an array of parts: `{type:"text"}`, `{type:"image_url", image_url:{url}}` (a `data:` URL is accepted), `{type:"input_audio", input_audio:{data, format}}`. llama-server + Ollama accept this shape.
- **llama.cpp multimodal (libmtmd)** — `llama-server -m model.gguf --mmproj mmproj.gguf` serves image/audio over `/chat/completions`; the **surest** path for the eventual E2E (ollama's multimodal support is narrower).
- **Gemma 3n** — the user's `gemma-4-e4b` = Gemma 3n E4B (native image/audio/video) — the intended E2E target once a server is up.

## Risks, Unknowns, Dependencies
- **Live-media E2E is heartbeat-gated (the known wall).** No multimodal server on the machine. *Mitigation:* the entire pipeline (Message media parts, OpenAI mapping, routing, gating) is **pure and unit-tested**; the live "does Gemma read this PNG" check is deferred to a human-set-up server (route TBD: `llama-server`+mmproj vs an ollama vision pull). This is a known checkpoint, not a blocker.
- **`Message` blast radius.** *Mitigation:* additive `media` field (not replacing `text`) keeps every existing `msg.text` reader and the trace schema working; a media-free Message serializes byte-identically to today.
- **Base64 in traces / memory.** *Mitigation:* traces store a media *descriptor* (`mime`, length, short hash), never the blob.
- **Capability honesty.** A media file routed at a text-only backend (mistral.rs) or a non-multimodal model must **degrade gracefully** (clear message / skip), never silently send bytes that get ignored.

## Recommended Approach
Ship the **multimodal input pipeline**, fully unit-tested, on the OpenAI valve; defer the live-media E2E:
1. **`Message` media parts** — additive `media: Vec<MediaPart{mime, data(base64)}>` + constructors; serde default-empty.
2. **OpenAI mapping** — `map_message` emits the content array for media, string otherwise (pure, tested).
3. **Modality gating** — `ModelProfile.modalities` (config, ADR-006) + honest `Capabilities.supports_media`; a pure `route_file(path) -> {AppendText | Media | Unsupported}` classifier.
4. **`ferric query --file`** — repeatable; text/code appended to the prompt (any model), media → gated `MediaPart`.
5. **Docs** — "any file" input + the multimodal-server requirement for media; timeline append.
6. **Deferred:** the live-media heartbeat (Gemma 3n reads an image/audio clip) — runs when a multimodal server is set up; recorded as the sprint's one human-verification checkpoint.

**Open product detail (does NOT block the build):** which multimodal-server route to stand up for the eventual E2E — `llama-server`+mmproj (surest, esp. audio/video) vs an ollama vision pull (simplest). Surface at the E2E checkpoint, not now.
