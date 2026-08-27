Finalized - DO NOT EDIT

# Sprint 10 Test Plan — Multimodal "any file" input

## Unit Tests (default CI, `cfg(test)`)
- **T-1001** (`ferric-core`): `Message` media roundtrip (serde, media-free == today); `classify_path` — `.rs/.md/.txt/.json`→Text, `.png/.jpg/.webp`→Media(Image), `.wav/.mp3/.flac`→Media(Audio), `.mp4/.mov`→Media(Video), `.xyz`→Unknown; `decide_attachment` — Text→AppendText, Media+declared+supported→Media, Media+undeclared→Skip(reason), Media+!supports_media→Skip(reason).
- **T-1002** (`ferric-provider`, `--features backend-openai`): `map_message` — media-free → string `content`; with media → parts array (`image_url` data-URL = `data:<mime>;base64,<data>`, `input_audio`); `Capabilities.supports_media` = true (openai) / false (mistral).

## Integration Tests
- **T-1003** (`ferric-cli`, `--mock`): `query --file <text.md>` folds the file text into the user message; `query --file <img.png>` with no `--modality` → the media is skipped and the reason is surfaced (stderr/trace), message has no `MediaPart`; `query --file <img.png> --modality image` → message carries one `MediaPart` with the right mime.

## End-to-End Tests
- **DEFERRED — the single human-verification checkpoint:** a live multimodal model (Gemma 3n via `llama-server --mmproj`, or an ollama vision pull) actually reading an attached image/audio clip end-to-end. Requires a multimodal-capable server the dev machine does not have (no `llama-server`; ollama has only text models pulled). Runs as a heartbeat once a multimodal server is stood up; recorded as **deferred** in the test-report — an explicit, not silent, gap. Server route (llama-server+mmproj vs ollama vision) is the open product detail surfaced at that checkpoint.

## Notes
- Backward-compat is a first-class assertion: a media-free `Message` must serialize byte-identically to the pre-sprint shape (the `#[serde(skip_serializing_if)]` guard), so existing traces/tests are untouched.
- No base64 blobs in traces — media logged as a descriptor (mime + length), asserted in the trace test if a trace event is added.
