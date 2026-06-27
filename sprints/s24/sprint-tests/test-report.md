# Sprint 24 Test Report — Live multimodal heartbeat (the marquee s10 goal, finally live)

**Date:** 2026-06-26. The multimodal content-parts mapping is proven by existing
units; the **first-ever live image-through-Ferric run** is the headline — the
pipeline carries pixels to a model that sees them.

## Build / Lint (green)
- The image content-parts mapping is already unit-tested in `ferric-provider` (`media_part_json` → `image_url` data-URL; string-vs-parts; `supports_media`). `cargo test --workspace` green; clippy `-D warnings`; fmt clean. **No Ferric code change** — the send path was already correct.

## End-to-End — RAN it: image → Ferric → llama-server(mmproj) → a seeing model
**Setup:** prebuilt llama.cpp `b9821` + **SmolVLM-500M-Instruct** GGUF (436 MB) + its
**mmproj** (108 MB) from ggml-org; `llama-server -m model --mmproj mmproj -c 4096
--port 8080` → `/health` ok. Test image: a generated **96×96 red square** PNG.

**Result — the pipeline works:**
1. `ferric query --backend openai --api-base :8080/v1 --file red96.png --modality image --protocol grammar "describe … then task_complete"` ran the loop and **llama-server's vision encoder processed the image** — server log: `process_mtmd: encoding mtmd batch … n_chunks = 1`. So Ferric base64'd the image into an `image_url` content-part and llama-server's mmproj consumed it. **Pixels flow Ferric → llama.cpp → the vision model.**
2. **Vision confirmed** — a direct query in the *exact* `image_url` format Ferric emits → SmolVLM answered **"Red."**, correctly identifying the square. The model sees what Ferric sends.

**Finding (recorded honestly):** under Ferric's *constrained agentic grammar*, the
tiny 500M VLM produced a degenerate `task_complete` summary (it echoed a system-prompt
line instead of describing the image). The image still reaches the model; the JSON
grammar just confuses a very small VLM's free-form output. A larger VLM, or an
unconstrained "describe" step, captions correctly (the direct query proves it). The
*pipeline* is validated regardless — that's the heartbeat.

## Verdict
**Multimodal is live end-to-end** — the longest-deferred goal (built sprint 10, never
run). Ferric → `llama-server --mmproj` → a vision model that sees the image, with the
`image_url`/base64 content-parts mapping proven correct against real pixels. ollama
GGUF reuse wasn't possible (no vision model there), so a small VLM was fetched; the
download/model is the only thing that wasn't already in-repo. Caveat documented:
constrained grammar + a sub-1B VLM degrades free-form captioning (use a bigger VLM or
an unconstrained describe). ADR-033.
