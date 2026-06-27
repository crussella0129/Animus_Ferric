# Sprint 24 Research Report — Live multimodal heartbeat (llama-server + mmproj)

> The deferred marquee goal since sprint 10: the multimodal *input pipeline*
> (`query --file --modality`) was built + unit-tested but **never run against a real
> vision model** — no multimodal server was available. Sprint 23 fixed that
> (llama.cpp/`llama-server` validated, `--mmproj` wired). Now: run an image through
> Ferric → llama-server → a model that can see it, end to end.

## Grounded findings
- **Send path is OpenAI-standard + compatible.** `ferric-provider/src/openai.rs:288` maps an image `MediaPart` to `{"type":"image_url","image_url":{"url":"data:<mime>;base64,<data>"}}` — exactly what `llama-server` (with `--mmproj`) accepts on `/v1/chat/completions`. Audio → `input_audio`. `Capabilities.supports_media = true` for the valve.
- **The full input pipeline exists (sprint 10, ADR-023/026):** `query --file <img> --modality image` → `classify_path`/`decide_attachment` → base64 → gated `MediaPart` → content-parts. Media attaches only when the modality is declared AND the backend carries media (the valve).
- **llama-server multimodal is proven reachable** (sprint 23: prebuilt `b9821` drove Ferric at 100%). It supports vision via `--mmproj <projector.gguf>` + the `image_url` API.
- **PR cadence clean:** PR #9 (sprint 23) merged → `main` has s23, `dev` clean.

## The gap: no vision model locally
ollama's models (llama3.2:1b, llama3.1:8b, qwen2.5-coder:7b) are **text-only** — no
mmproj. Need a small **vision GGUF + its mmproj**. Smallest viable options (GGUF +
mmproj published by ggml-org / the model authors):
- **SmolVLM-500M-Instruct** (~0.5–1 GB total) — tiniest; ggml-org publishes GGUF + mmproj. **Recommended** for a fast heartbeat.
- moondream2 (~1.8 GB), Qwen2-VL-2B (~1.5 GB) — fallbacks if SmolVLM misbehaves.

## Decisions Reviewed
- **ADR-023 / ADR-026** — multimodal content model + gating (media only when declared + backend-capable). Unchanged; this validates it live.
- **ADR-032** — llama-server is the engine; `--mmproj` is the documented media path.
- **ADR-001/005** — the constrained valve, loopback-bound. The multimodal run still goes through it.

## Design (the heartbeat)
1. Download a small vision GGUF + mmproj (SmolVLM-500M) to a scratch dir.
2. `llama-server -m <vlm.gguf> --mmproj <mmproj.gguf> -c 4096 --host 127.0.0.1 --port 8080`.
3. Generate a tiny known test image (e.g., a solid-color square / simple shape) — no asset dependency.
4. `ferric query --backend openai --api-base :8080/v1 --protocol grammar --file test.png --modality image "describe the image in one sentence, then call task_complete with that description"`.
5. **Verify (AI-checkable):** the trace shows the image went out as a content-part, and the model's response/`task_complete` summary **references the image content** (e.g. names the color/shape) — proving the pipeline carries pixels to a model that sees them. Accuracy is a soft human check; "did it see *an* image" is verifiable.

## Risk
- **Download + model choice** — gated via plan approval (like sprint 23's llama.cpp fetch). If SmolVLM's GGUF/mmproj pairing won't load in `b9821`, fall back to moondream2; if all fail, the pipeline assertion (image leaves Ferric as a content-part) still lands and the live model run is deferred with exact steps.
- **Constrained + vision interplay** — the image is *input*; the grammar constrains *output*. A "describe then task_complete" framing keeps it inside the agentic loop. If the constraint conflicts with the VLM, also try `--protocol native`/no-constraint for the describe step and record the finding.

## Recommended approach
T-2401: live multimodal heartbeat — fetch SmolVLM GGUF+mmproj, run llama-server
`--mmproj`, send a generated test image via `ferric query --file --modality image`,
verify the image reaches a seeing model + record the response. T-2402: ADR-033
(multimodal validated end-to-end) + docs (the `--mmproj` heartbeat in docs/llama-cpp.md
+ docs/multimodal.md). One PR per sprint ([[one-pr-per-sprint]]); `dev` is clean.
