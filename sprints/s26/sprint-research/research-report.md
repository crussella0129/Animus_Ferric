# Sprint 26 Research Report — Gemma 4 E4B audio modality (the other half of multimodal)

> Sprint 24/25 validated **vision** end-to-end. Gemma 4 E4B also has a native **audio**
> encoder. This sprint validates the **audio** path — completing Ferric's multimodal
> story — using the cached Gemma 4 model (no re-download).

## Grounded findings
- **Ferric already sends audio correctly.** `ferric-provider/src/openai.rs:media_part_json` maps an `audio/*` `MediaPart` → an OpenAI **`input_audio`** content block (`{data: base64, format}`, `audio/mpeg`→`mp3` else the subtype, e.g. `wav`). The `--file/--modality audio` routing (ADR-023) is the same pipeline as vision.
- **llama.cpp supports audio input**, and specifically **Gemma 4 audio via a Conformer encoder** (PR #21421); **llama-server accepts the `input_audio` content block** on `/v1/chat/completions`. Gemma 4's mmproj is a unified vision+audio projector (`gemma4uv`). My prebuilt **`b9821`** logs `init_audio: audio input is in experimental stage` when loading the Gemma 4 mmproj — **audio is live, no update needed.**
- **PR cadence clean:** PR #11 (sprint 25) merged → `main` has s25, `dev` clean. Gemma 4 cached at `/tmp/s25gemma`.

## Decisions Reviewed
- **ADR-035** — Gemma 4 E4B is the reference model; this validates its audio modality. **ADR-023/026** — multimodal content-parts + gating (reused as-is; `input_audio` already mapped). **ADR-032** — llama-server engine.

## Validation (done in research — cached model, local TTS, no download)
1. Started `llama-server -m gemma-4-E4B … --mmproj …` (cached) → `init_audio` initialized.
2. Generated a **known-speech WAV** via Windows TTS (16 kHz mono): *"The quick brown fox jumps over the lazy dog."* — no asset/download.
3. `ferric query --file speech.wav --modality audio --protocol grammar "transcribe … then task_complete"` →
   **`task_complete("The quick brown fox jumps over the lazy dog.")`** — an exact transcription, inside the constrained agentic loop.

So the **audio modality works end-to-end**: Ferric → `input_audio` → Gemma 4's Conformer → accurate ASR, under the agentic grammar. **No Ferric code change** (the s10 pipeline already handled `input_audio`).

## Risk — none material
- Audio is "experimental" in llama.cpp (the server warns) but worked cleanly here. Quality may vary by audio; the *pipeline* + the reference model's ASR are validated.

## Recommended approach
This is a validation sprint (like s24/s25): no code change. T-2601 records the audio
validation; T-2602 = ADR-036 (Gemma 4 audio modality validated; Ferric multimodal =
vision + audio, both live) + docs (the `--modality audio` walkthrough in
docs/llama-cpp.md / docs/multimodal.md); README Status 26 + Sprint 26 timeline. One
PR per sprint ([[one-pr-per-sprint]]); `dev` clean.
