# Artifact: 100%-Rust Feasibility Assessment (mid-2026)

> Source: web-research agent, 2026-06-10. Full URL list at bottom.

## Verdict

**Yes — viable today**, with two honest asterisks: (1) GPU kernels are still CUDA/Metal source compiled into the binary (~18% of mistral.rs repo is non-Rust kernel code); (2) Windows-native CUDA builds of the pure-Rust stacks are rougher than llama.cpp (WSL2 recommended for CUDA; CPU-only native works). The harness layer — tokenizers, grammar enforcement, GGUF parsing, PTY, TUI, SQLite — is fully Rust-native with no compromises.

## Inference options

### (a) mistral.rs (recommended pure path) — v0.8.3, ~7.3k★, very active
- GGUF 2–8 bit, GPTQ/AWQ/HQQ/FP8 + **ISQ** (in-situ quantization of any HF safetensors — llama.cpp can't).
- CUDA (FlashAttention V2/V3, CUDA graphs, paged attention), Metal, CPU (MKL/Accelerate). **No Vulkan** (AMD/Intel GPU on Windows → CPU fallback or llama-server escape valve).
- Best-in-class Rust agentic features: integrated tool calling with grammar enforcement + strict schema mode, **llguidance merged** (regex/JSON-Schema/Lark CFG constrained decoding), MCP client.
- Perf: CUDA decode parity ±10% vs llama.cpp (project benchmarks: 86 vs 82 t/s A10, 127 vs 137 A100); CPU/Metal somewhat behind. For agentic use, noise.
- Risks: single-lead maintainer (bus factor); model-coverage lag (weeks vs llama.cpp's days); Windows CUDA build friction (issues #847, #1122, #749, #1099).

### (b) llama-cpp-2 FFI bindings — v0.1.146, tracks upstream in lockstep
- Gains llama.cpp coverage + Vulkan + best CPU perf, GBNF in-process.
- Admits entire ggml/llama.cpp C++ surface into the process; bindings self-described as UB-capable; clang+bindgen+CMake on Windows; small community (581★). Least "Rust" philosophically.

### (c) Rust harness + external server (llama-server / Ollama HTTP)
- Zero FFI, crash isolation, swap backends freely; llama-server exposes GBNF + JSON-schema response_format + tool calling.
- Loses logit-level access (constrained decoding limited to what server exposes); ships a C++/Go binary outside the ownership chain; known llama-server tool-call compat bugs (#22072, #20198).

### Others
- **Candle** (HF, ~20.5k★): tensor framework, not an inference server — you'd own KV-cache/batching/sampling/templates yourself; that's what mistral.rs already built on Candle.
- **Burn/CubeCL + Burn-LM**: the only stack where kernels themselves are Rust; Burn-LM real but early — watch for 2027.
- **kalosm**: Candle-based, structured generation, thin community. **ratchet**: browser-first. **rustformers/llm**: unmaintained, avoid.

## Constrained decoding — solved in Rust

- **llguidance** (Rust core, ~50µs/token masks; JSON Schema, regex, Lark CFG) — integrated into mistral.rs AND llama.cpp AND vLLM/SGLang. The strongest single enabler: one grammar abstraction spanning every backend option.
- **outlines-core** (Rust core, FSM/index) — heavier precompute alternative.

## Supporting crates — all green

| Need | Crate | Note |
|---|---|---|
| Tokenization | `tokenizers` (HF) | Rust-native, active |
| GGUF parsing | `gguf-rs` / Candle reader | mmap, type-safe |
| Code parsing | `tree-sitter` | C core via cc — ubiquitous, fine |
| PTY (Windows) | `portable-pty` | ConPTY; resize quirk patched in `portable-pty-psmux` fork |
| TUI | `ratatui` 0.30.x | de-facto standard |
| SQLite | `rusqlite` | bundled, no system dep |

## Recommended architecture

Inference **trait** with two backends: **mistral.rs in-process as the flagship pure-Rust backend** + **OpenAI-compatible HTTP as the compatibility valve** (llama-server for Vulkan/AMD users, Ollama for convenience). Skip (b) — it buys coverage obtainable via (c) without importing C++ UB into the agent process. Use **llguidance grammars** as the constrained-decoding abstraction across both. Windows support matrix: CPU native + WSL2-CUDA for mistral.rs; AMD GPUs routed to llama-server backend.

Maintenance risk (low→high): llama.cpp < Candle/tokenizers (HF-backed) < mistral.rs (single lead) < llama-cpp-2 < Burn-LM/ratchet/kalosm.

## Sources

1. https://github.com/EricLBuehler/mistral.rs
2. https://github.com/huggingface/candle
3. https://github.com/guidance-ai/llguidance
4. https://github.com/utilityai/llama-cpp-rs
5. https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md

Extras: mistral.rs discussions/612 (CUDA benchmarks), issues #1122/#847 (Windows CUDA), #903 (Metal vs MLX); tracel-ai/burn + burn-lm; floneum/floneum (kalosm); dottxt-ai/outlines-core; huggingface/tokenizers; lib.rs/crates/gguf-rs; lib.rs/crates/portable-pty-psmux; llama.cpp issues #22072/#20198; rustformers/llm (unmaintained); huggingface/ratchet.
