# Agent Tasks (Persistent Backlog)

> Sprint 23 (llama.cpp first-class) is **done** — validated Ferric on full
> `llama-server` for the first time: the constrained loop runs at **100% Ring-0
> fire rate, identical to ollama**, via an ollama GGUF blob (no re-download). It's
> now the recommended engine (ollama = one-flag fallback); ADR-032 + docs/llama-cpp.md.
> Wide context (`-c`), multimodal (`--mmproj`), and a single edge-ready binary are
> proven/documented. The launcher needed no code change (already contract-tested).

Open candidates (sprint 24+):
- **Live multimodal heartbeat** — now unblocked: `llama-server --mmproj <proj.gguf>` + a vision GGUF → run `ferric query --file img.png --modality image` end-to-end (ADR-026 follow-on).
- **A no-progress / max-same-tool guard** for "semantic flailing" (ADR-031's L2 mode the repetition guard misses).
- **Harder bench levels (L7+)**; more Ring-2 tools (apply_patch); MCP-stdio (ADR-012, needs the ADR-005 call).
- **Edge run** — actually run on a Jetson Orin Nano / Pi (CUDA/arm64 build) to confirm the minimal footprint (human-gated on hardware).
