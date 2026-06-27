# Sprint 26 Test Report — Gemma 4 E4B audio modality (validated)

**Date:** 2026-06-27. Audio works end-to-end — Ferric multimodal is now **vision +
audio**, both live on the reference model.

## Build / Lint (green)
- No Ferric code change (validation sprint). `cargo test --workspace` green; clippy `-D warnings`; fmt clean.

## End-to-End — RAN it: speech → Ferric → Gemma 4 → exact transcription
- **Setup:** the **cached** Gemma 4 E4B (no re-download) on the prebuilt `b9821` llama-server — the log shows `init_audio: audio input is in experimental stage` (Gemma 4's Conformer audio encoder loaded via the unified mmproj).
- **Test audio:** Windows TTS → a 16 kHz-mono WAV of *"The quick brown fox jumps over the lazy dog."* — known ground truth, no asset/download.
- **Result:** `ferric query --backend openai --api-base :8080/v1 --file speech.wav --modality audio --protocol grammar "transcribe … then task_complete"` →
  **`task_complete("The quick brown fox jumps over the lazy dog.")`** — an **exact transcription, in 1 turn, inside the constrained agentic loop.**

The model's `task_complete` summary matches the synthesized speech verbatim ⇒ Ferric's
`input_audio` content-part carried the audio to Gemma 4's Conformer, which transcribed
it under the JSON grammar. **No Ferric code change** — the sprint-10 `media_part_json`
audio mapping was already correct.

## Verdict
**Audio modality validated.** Combined with vision (s24/25), **Ferric multimodal is
vision + audio**, both proven end-to-end on the reference model (Gemma 4 E4B) via one
llama.cpp binary. This completes the Gemma-4-as-reference picture: ~4B, agentic (L5),
fully multimodal. Audio is "experimental" upstream but clean on clear speech. No
human-verification checkpoint (the transcription is an exact match). ADR-036.
