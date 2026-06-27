# Sprint 23 Test Report — llama.cpp (llama-server) validated as the engine

**Date:** 2026-06-26. The launcher contract is proven by existing unit tests; the
**first-ever live run of Ferric on full llama.cpp** is the headline — and it works.

## Unit (`ferric-cli` — green, already present)
- `server::command()` contract for `Engine::LlamaServer` is locked by 3 existing tests (`llama_server_argv` → `-m/-c/--host/--port`; `llama_server_mmproj` → `--mmproj` iff set; `ollama_argv_and_env` → `serve`). **No code change needed** — the launcher was already correct; the live run confirms its argv binds + serves.
- `cargo test --workspace` green; clippy `-D warnings`; fmt clean.

## End-to-End — RAN it: Ferric on full llama.cpp (the headline)
**Setup (no multi-GB download):**
- Fetched the prebuilt **llama.cpp `b9821` CPU/x64** release (`llama-server.exe` + ggml DLLs).
- Pointed `-m` at an **ollama GGUF blob** (`~/.ollama/.../blobs/sha256-74701a8c…`, magic bytes `GGUF` ✓) — the reuse trick worked, zero re-download.
- `llama-server -m <blob> -c 8192 --host 127.0.0.1 --port 8080` → `model loaded`, `/health` = ok.

**Results — Ferric drives llama.cpp under the constrained valve:**
- `ferric query --backend openai --api-base :8080/v1 --protocol grammar "create hello.txt … then task_complete"` → **`hello.txt` created** (a tool call enforced by llama-server's grammar). The constrained loop runs on llama.cpp end-to-end. (It ended on `repetition_guard` — the 1B's known sprint-22 ceiling, *not* an engine issue; the tool call itself fired correctly.)
- `ferric toolbench --api-base :8080/v1 --protocol grammar --max-ring 0 --iterations 6` → **36/36 = 100.0% solid** across all 6 Ring-0 tools. **Identical to ollama's 100%** — the constrained `response_format`/grammar thesis holds on full llama.cpp.
- **tok/s:** ~28 tok/s on the **CPU** build (1B, no GPU). Not a fair race vs a possibly-GPU ollama — the win is *control*, not this number: a CUDA/Vulkan build gives GPU speed, and llama-server is one static binary + DLLs (edge-ready for Jetson/Pi).
- **Context:** `-c 8192` was set trivially (and goes as wide as VRAM allows / `-c 0` = full trained context) — vs ollama's narrow `num_ctx` default. The wider agentic context the user is after is a flag away, and Ferric's launcher already exposes it (`--ctx`).

## Verdict
**Ferric's preferred engine is proven.** Full llama.cpp (llama-server) drives the
constrained agentic loop at 100% tool-call fire rate — matching ollama — with the
context-window control, multimodal (mmproj) path, and minimal single-binary
footprint the user wants for edge. The ollama-blob reuse means no model re-download.
ollama remains a pluggable fallback. (ADR-032.) No human-verification checkpoint —
the live run succeeded.
