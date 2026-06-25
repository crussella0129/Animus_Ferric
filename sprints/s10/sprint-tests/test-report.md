# Sprint 10 Test Report — Multimodal "any file" input

**Date:** 2026-06-24 · The multimodal input pipeline is fully built and tested at
the unit + integration level; the single live-media check is deferred (no
multimodal server on the machine — the planned, explicit checkpoint).

## Unit Tests (default CI — all green)
- **`ferric-core::media`** — `classify_by_extension` (text/image/audio/video/unknown, case-insensitive); `text_always_appends`; `media_needs_declared_and_supported` (declared+supported→Media, undeclared→Skip, text-only-backend→Skip); `unknown_skips`; `parse_modalities_tolerant`; **`base64_rfc4648_vectors`** (the canonical `""`/`f`/`fo`/`foo`/`foob`/`fooba`/`foobar` vectors, both padding cases).
- **`ferric-core::message`** — `media_message_roundtrip` + the backward-compat assertion that a media-free message serializes with **no `media` key** (byte-identical to the pre-sprint schema); legacy `{"role","text"}` JSON still parses.
- **`ferric-provider::openai`** (`--features backend-openai`) — `map_message_text_only_is_string`; `map_message_media_is_parts_array` (`text` part + `image_url` data-URL + `input_audio` with format); `capabilities_supports_media`.

## Integration Tests (`ferric-cli`, `--mock`, all green)
- **`query_file_text_folds_into_prompt`** — `ferric query --mock --file notes.md` succeeds and the folded file text shows up in the assembled prompt's `chars` count (≥ the file length).
- **`query_file_media_skipped_with_reason`** — `ferric query --mock --file photo.png` (no multimodal backend) succeeds **non-fatally** and prints a `skip … photo.png …` reason to stderr — surfaced, never silent.

## Cross-feature build / lint
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all` clean.
- Compiles green under **default**, **backend-openai**, and **backend-mistralrs** (every `Message{}` and `Capabilities{}` site threaded; the loop's `RunArgs.media` carried through all 4 sites + both `drive_*` paths).

## End-to-End — DEFERRED (the single human-verification checkpoint)
- **Live media:** a real multimodal model (Gemma 3n via `llama-server --mmproj`, or an ollama vision pull) actually reading an attached image/audio clip — i.e. `ferric query --backend openai --file diagram.png --modality image "describe"` against a multimodal server, and confirming the model's answer reflects the image.
- **Why deferred:** the dev machine has no multimodal-capable server (no `llama-server`; ollama has only text models pulled). The entire pipeline beneath this — file routing, gating, base64, the OpenAI parts array, media threading into the loop — is unit/integration-tested, so what remains is purely "does the model read the bytes," which needs the server.
- **To run it later:** stand up `llama-server -m gemma-3n-E4B.gguf --mmproj mmproj.gguf` (or pull an ollama vision model), then the command above. Recorded here as an explicit open item, not a silent gap.

## Verdict
Sprint 10 ships a complete, tested multimodal **input** pipeline. AI-verifiable
scope is **green**; the one un-AI-verifiable piece (a real model reading media) is
deferred to a heartbeat with a clear runbook.
