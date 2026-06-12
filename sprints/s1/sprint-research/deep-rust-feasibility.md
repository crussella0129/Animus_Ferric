# Artifact: Deep-Rust Ownership Graph — CUDA + tree-sitter (mid-2026)

> Source: web-research agent, 2026-06-10.

## Rust-native CUDA

- **Rust-CUDA** (rustc_codegen_nvvm): revived Jan 2025, but nightly-pinned, last crates.io release 2022, blog quiet since Aug 2025. Avoid for production.
- **NVlabs cuda-oxide** (v0.1.0, May 2026): NEW — standard Rust → PTX via Pliron (pure-Rust MLIR-like IR); "entire compiler builds with cargo — no C++ toolchain." The most ownership-clean kernel pipeline in existence, but explicit alpha. **Watch closely; don't build on it yet.**
- **CubeCL + Burn** (tracel-ai, $3M funded): `#[cube]` Rust kernels → CUDA/ROCm/Metal/Vulkan/WGSL/CPU-SIMD. v0.10 (May 2026); Burn v0.21 (May 2026, 8× lower overhead). The only all-Rust-kernel path that ships. Caveat: host side uses cudarc, and lowering emits CUDA-C++ text JIT'd by NVRTC — no hand-written C++ in YOUR tree, which is the real win. **Burn-LM is v0.0.1** (Llama 3.x 1B–8B only) — not a viable general GGUF engine yet; re-evaluate in 6–12 months.
- **cudarc**: mature de-facto standard bindings (CUDA 11.4–13.0, dynamic loading — good for Jetson JetPack); used by candle/mistral.rs/CubeCL.
- **Jetson Orin (aarch64)**: cudarc dynamic-load + candle/mistral.rs community runs exist (compute 8.7); nobody CI-tests Jetson — verify on-device.

**Verdict:** practical now = mistral.rs/candle on cudarc (document hand-written CUDA C++ kernels as a boundary). Purest future = Burn/CubeCL once Burn-LM matures; cuda-oxide is the long-term compilation pipeline. On any NVIDIA path the irreducible floor is the proprietary driver/NVRTC — honest framing: **"Rust down to the driver ABI."**

## Rustifying tree-sitter

| Option | Reality | Ownership status |
|---|---|---|
| C core via `cc` (official crate) | status quo; ~15K LOC, fuzzed, stable | documented FFI boundary |
| helix-editor/tree-house | better bindings (Helix 25.07) — same C core | not a rustification |
| tree-sitter-c2rust | machine-transpiled runtime; cargo-pure | unaudited machine-generated `unsafe` Rust; mainstream grammars NOT shipped — worse for accountability |
| rust-sitter | proc-macro for grammars YOU define | useless for existing languages |
| wasm grammar path (wasmtime) | grammars sandboxed as wasm; core stays C | optional later upgrade for untrusted grammars |
| syn / rowan / cstree | pure Rust, production grade | Rust-language only |

**Verdict: "pure Rust multi-language parsing" is not realistic in 2026.** Accept the tree-sitter C core + generated parsers as a **named, versioned, fuzz-tested FFI boundary** in the ownership graph; use `syn` for deep Rust-language analysis; consider wasm-sandboxed grammars later for third-party languages.

## Other non-Rust residue in a GGUF harness

- CPU-only: **~100% Rust above the OS is achievable today** — candle's default CPU matmul is the pure-Rust `gemm` crate; HF `tokenizers` is Rust; GGUF parsing is Rust; just don't enable mkl/accelerate features.
- NVIDIA GPU: proprietary driver (libcuda) + NVRTC are unavoidable; cuBLAS/cuDNN partially avoidable (CubeCL matmul claims parity).
- llama.cpp FFI: avoided entirely (ADR-001).

## Sources

1. https://github.com/NVlabs/cuda-oxide
2. https://rust-gpu.github.io/blog/2025/01/27/rust-cuda-reboot/
3. https://burn.dev/blog/ (+ github.com/tracel-ai/burn-lm)
4. https://github.com/coreylowman/cudarc
5. https://github.com/helix-editor/tree-house
Extras: github.com/Rust-GPU/Rust-CUDA; github.com/tracel-ai/cubecl; github.com/shadaj/tree-sitter-c2rust; github.com/hydro-project/rust-sitter; docs.rs/tree-sitter (WasmStore); github.com/huggingface/candle; phoronix.com/news/NVIDIA-CUDA-Oxide-0.1.
