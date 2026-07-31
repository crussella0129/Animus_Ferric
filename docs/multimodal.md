# Multimodal "any file" input

`ferric query` can take **any file** as input with `--file` (repeatable). What
happens to a file depends on what it is and what your model can read.

```sh
# Text/code files fold into the prompt — works on ANY model:
ferric query --file src/lib.rs --file notes.md "explain what this code does"

# Media files (image/audio/video) attach as content parts — needs a model that
# can read them, declared with --modality:
ferric query --file diagram.png --modality image "describe the diagram"
```

## How a file is routed

Each `--file` is classified by extension and then gated — pure logic in
`ferric-core::media` (`classify_path` → `decide_attachment`):

| File kind | What happens | Needs |
|---|---|---|
| **Text / code** (`.rs .md .txt .json .toml .py …`) | read and folded into the prompt as text | nothing — works on any model |
| **Image** (`.png .jpg .webp .gif`) | attached as an `image_url` content part (base64 data URL) | `--modality image` + a multimodal model on the OpenAI valve |
| **Audio** (`.wav .mp3 .flac .ogg`) | attached as an `input_audio` part | `--modality audio` + a multimodal model |
| **Video** (`.mp4 .mov .webm`) | attached as an `image_url` part (best-effort; server-dependent) | `--modality video` + a multimodal model |
| **Unknown** extension | skipped | — |

**Gating (ADR-006 / ADR-022).** Media is attached only when **(a)** you declared
its modality with `--modality` (explicit config — Ferric never sniffs a model's
abilities) **and (b)** the backend can carry media (`supports_media` — true for
the OpenAI valve, false for a text-only backend). If either is
missing, the file is **skipped with a reason printed to stderr** — never sent
silently and never a hard failure. So `--file photo.png` with no `--modality`
runs fine, text-only, and tells you why the image wasn't sent.

## Running media end-to-end

The text path needs no setup. For **media**, you need a multimodal-capable
server and model behind the OpenAI valve — e.g. Gemma 3n (`gemma-4-e4b`):

```sh
# llama.cpp with a projector (the surest path; supports image/audio):
ferric server up --engine llama-server --model gemma-3n-E4B.gguf --mmproj mmproj.gguf
ferric query --file clip.wav --modality audio "transcribe this"
ferric server down
```

(Ollama can serve some vision models too; pull one and point `--api-base`
at it. Audio/video coverage is widest on llama.cpp's libmtmd.)

## Notes
- Media bytes are base64'd into the request; **traces never store the blob** —
  only the message shape. Keep attachments reasonably sized for small contexts.
- `--modality` takes a comma list: `--modality image,audio`.
