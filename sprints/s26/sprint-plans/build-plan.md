Finalized - DO NOT EDIT

# Sprint 26 Build Plan — Gemma 4 E4B audio modality (validated)

Complete Ferric's multimodal story: after vision (s24/25), validate **audio**. Done
live in research (cached Gemma 4, local TTS WAV): Ferric `--modality audio` →
`input_audio` → Gemma 4's Conformer → exact transcription, inside the constrained
loop. No Ferric code change. Rationale: `sprints/s26/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** audio modality validated + documented.
  - **A. audio validation** — T-2601 (done in research)
  - **B. ADR-036 + docs** — T-2602

## Execution Sequence

### T-2601: Audio validation (done)
- llama-server on cached Gemma 4 (`init_audio` ok) + a Windows-TTS speech WAV → `ferric query --file speech.wav --modality audio --protocol grammar` → `task_complete("The quick brown fox jumps over the lazy dog.")` (exact ASR). Recorded in the test-report.

### T-2602: ADR-036 + docs
- **Touches:** `decisions.md`, `docs/llama-cpp.md`, `docs/multimodal.md`, `README.md`
- **Success:** ADR-036 (audio modality validated; Ferric multimodal = vision + audio, both live on Gemma 4; no code change). Docs: a `--modality audio` walkthrough; README Status 26 + Sprint 26 timeline.

## Post-build (test)
- workspace green (no code change) + the live audio transcription (done).
