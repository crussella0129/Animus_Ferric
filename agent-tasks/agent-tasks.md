# Agent Tasks (Persistent Backlog)

> Sprint 26 (Gemma 4 audio modality) is **done** — validated audio end-to-end: a
> Windows-TTS speech WAV → `ferric query --file speech.wav --modality audio` →
> `llama-server` (Gemma 4 Conformer, `init_audio` on b9821) → `task_complete("The
> quick brown fox jumps over the lazy dog.")` (exact ASR, inside the constrained
> loop). Ferric multimodal is now **vision + audio**, both live on the reference
> model. No Ferric code change (ADR-036). PR cadence clean.

Open candidates (sprint 27+):
- **GPU / edge run** — a CUDA llama.cpp build (or Jetson Orin Nano) to clear the s25 CPU timeouts + confirm the edge footprint; Gemma 4 might then reach L6.
- **A no-progress / max-same-tool guard** for "semantic flailing" (ADR-031).
- **Harder bench levels (L7+)**; more Ring-2 tools (apply_patch); MCP-stdio (ADR-012).
- `--chat` plain-LLM mode (deferred — a capable model removed the urgency).
- Audio quality on real (non-TTS) audio; video modality.
