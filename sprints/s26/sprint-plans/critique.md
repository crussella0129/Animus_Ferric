# Plan Critique — Sprint 26

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Validation sprint with no Ferric code change
- **Failure mode:** thin-sprint
- **Response:** same shape as s24/s25 — high-value validation of a never-run path. It **completes the multimodal story** (vision + audio, both live on the reference model) and is AI-verifiable: the transcription exactly matches the synthesized speech. The `input_audio` mapping was already correct (s10), so the deliverable is the evidence + the ADR/docs.

### C-002: Audio is "experimental" in llama.cpp
- **Failure mode:** flaky-feature
- **Response:** the server warns, but it worked cleanly (exact ASR). The claim is scoped: the *pipeline* + the reference model's ASR are validated on a clean 16 kHz mono WAV; quality may vary by audio — stated honestly in the ADR.

### C-003: Self-generated audio (TTS) might be unrealistically clean
- **Failure mode:** weak-test
- **Response:** TTS gives **known ground truth** (exact expected transcription) with no asset/download — ideal for a heartbeat. It proves the end-to-end path carries audio to a model that transcribes it; real-world ASR quality is a separate, model-level concern.

### C-004: One-PR-per-sprint cadence
- **Failure mode:** process-miss
- **Response:** `dev` clean (PR #11 merged). Close: push visible (no `-q`), verify `origin/main..dev` = s26 only, verify PR count — per [[one-pr-per-sprint]].

## Confidence
`clean` — the audio path was already built + tested at the mapping level; the live run (cached model, local TTS, no download) gives an exact-match transcription. ADR + docs are the deliverable.
