Finalized - DO NOT EDIT

# Sprint 24 Test Plan — Live multimodal heartbeat

## Build / Lint (default CI)
- `cargo test --workspace` green (the multimodal content-parts mapping is already unit-tested in `ferric-provider` — `media_part_json` → `image_url` data-URL; string-vs-parts; `supports_media`); `clippy --workspace --all-targets -- -D warnings`; `fmt --check`.

## End-to-End — RUN it (the heartbeat: image → seeing model)
1. `llama-server -m SmolVLM-500M-Q8.gguf --mmproj mmproj-SmolVLM-500M-Q8.gguf -c 4096 --host 127.0.0.1 --port 8080` (background); poll `/health`.
2. Generate a tiny known test image — a solid-color square (e.g. a red PNG) — so the expected content is unambiguous.
3. `ferric query --backend openai --api-base http://127.0.0.1:8080/v1 --model smolvlm --file /tmp/red.png --modality image --protocol grammar --workspace <tmp> "describe the image in one sentence, then call task_complete with that description"`.
4. **Assertions:**
   - **Pipeline (AI-verifiable):** the `q-*.jsonl` trace's assembled request carried the image as a content-part (the prompt/media went out) — proves Ferric base64'd + routed the image.
   - **Vision (heartbeat):** the model's completion / `task_complete` summary **references the image** — e.g. names the colour ("red") or "square"/"image". A response that mentions the actual content ⇒ pixels reached a model that saw them.
5. If `--protocol grammar` fights the VLM (no/garbled output), re-run with `--protocol native` and record which works.

## Fallback (honest)
- If neither SmolVLM nor moondream2 loads in llama.cpp `b9821`: the multimodal unit tests + a trace assertion (image left Ferric as a content-part) still land; the live model run is **deferred** with the exact `--mmproj` commands (the download/model is the only gap, not Ferric).

## Notes
- "Did the model see *an* image and describe its colour/shape" is AI-verifiable; exact caption quality is a soft human check. The heartbeat is: the pipeline carries pixels to a seeing model, served by llama.cpp.
