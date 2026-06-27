# Agent Tasks (Persistent Backlog)

> Sprint 24 (live multimodal heartbeat) is **done** — the multimodal pipeline (built
> s10, never run) is **validated end-to-end**: a red square → `ferric query --file
> --modality image` → `llama-server --mmproj` (SmolVLM-500M) → the vision encoder
> processed it; the model answered "Red." The `image_url`/base64 mapping is proven;
> no Ferric code change. ADR-033. Caveat: a sub-1B VLM degrades under the JSON grammar.

Open candidates (sprint 25+):
- **Relax the constraint for a vision turn** — a `--modality`-aware option to drop the JSON grammar on a describe step (the ADR-033 caveat: tiny VLMs caption poorly under the tool-call grammar). Or a bigger VLM.
- **A no-progress / max-same-tool guard** for "semantic flailing" (ADR-031's L2 mode the repetition guard misses).
- **Harder bench levels (L7+)**; more Ring-2 tools (apply_patch); MCP-stdio (ADR-012, needs the ADR-005 call).
- **Actual edge run** — Jetson Orin Nano / Pi (CUDA/arm64 build) to confirm the minimal footprint live (human-gated on hardware).
