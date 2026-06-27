# Agent Tasks (Persistent Backlog)

> Sprint 25 (validate Gemma 4 E4B) is **done** — Gemma 4 E4B is Ferric's **reference
> ~4B multimodal model** (ADR-035). Live on llama.cpp: **measured_level 5** (= the 8B,
> above the 1B's none → confirms the ~4B agentic floor); **multimodal inside the
> constrained loop** (`task_complete "a solid red rectangle"`) → closes ADR-033 with
> no harness change; Ring-0 toolbench 100% solid. No Ferric code change. (Caveat: use a
> GPU llama.cpp build — CPU timed out L0.)

Open candidates (sprint 26+):
- **Gemma 4 E4B audio modality** — it has a native audio encoder; test `ferric query --file clip.wav --modality audio` (the pipeline already supports `input_audio`).
- **GPU / edge run** — a CUDA build (or Jetson Orin Nano) to clear the CPU timeouts + confirm the edge footprint; Gemma 4 might then reach L6.
- **A no-progress / max-same-tool guard** for "semantic flailing" (ADR-031).
- **Harder bench levels (L7+)**; more Ring-2 tools (apply_patch); MCP-stdio (ADR-012).
- `--chat` plain-LLM mode (deferred — a capable model removed the urgency).
