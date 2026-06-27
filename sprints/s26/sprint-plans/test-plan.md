Finalized - DO NOT EDIT

# Sprint 26 Test Plan — Gemma 4 E4B audio modality

## Build / Lint (default CI)
- No Ferric code change (validation sprint). `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings`; `fmt --check`.

## End-to-End — RAN it (the audio heartbeat)
1. `llama-server -m gemma-4-E4B_q4_0-it.gguf --mmproj gemma-4-E4B-it-mmproj.gguf -c 8192 --port 8080` (cached) → log shows `init_audio` (Gemma 4 audio Conformer loaded on `b9821`).
2. Windows TTS → a known-speech WAV (16 kHz mono): *"The quick brown fox jumps over the lazy dog."*
3. `ferric query --backend openai --api-base :8080/v1 --file speech.wav --modality audio --protocol grammar "transcribe … then task_complete"` → **`task_complete("The quick brown fox jumps over the lazy dog.")`** — exact transcription, inside the constrained agentic loop.

**Assertion (AI-verifiable):** the model's `task_complete` summary matches the synthesized speech ⇒ Ferric's `input_audio` content-part carried the audio to Gemma 4's Conformer, which transcribed it under the grammar.

## Result
Audio modality **validated end-to-end**. Ferric multimodal is now **vision + audio**,
both live on the reference model (Gemma 4 E4B) via llama.cpp. No code change (the s10
`input_audio` mapping was already correct).
