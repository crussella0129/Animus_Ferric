# Sprint 4 Research Report: Multi-Backend Provider Architecture

## Problem Statement

Animus Ferric's only real backend is `MistralRsProvider`, which embeds the `mistralrs 0.8.1` Rust crate as an in-process inference engine. When we attempted to load `google/gemma-4-e4b` (a safetensors model), the engine returned:

```
Unsupported Hugging Face Transformers -CausalLM model class `Gemma4ForConditionalGeneration`
```

The root cause: `mistralrs 0.8.1` (the latest version published to crates.io as of June 20, 2026) does not include the `Gemma4ForConditionalGeneration` architecture blueprint. The Gemma 4 support was merged to the GitHub `master` branch (commit "Implement the new Google model (#2046)") *after* the 0.8.1 crate was published on April 2, 2026.

**This is a systemic problem, not a one-time bug.** Every time a new model family is released (Llama 4, Phi-4, etc.), Ferric will be blocked until `mistralrs` publishes a new crate version containing that architecture. This makes Ferric model-locked rather than model-agnostic.

---

## Option Analysis

### Option 1: Bump `mistralrs` to git `master`

**What it does:** Change `Cargo.toml` from `mistralrs = "=0.8.1"` to `mistralrs = { git = "https://github.com/EricLBuehler/mistral.rs", branch = "master" }`.

**Pros:**
- Zero architectural changes to Ferric's codebase
- Gemma 4 support is already in `master` (confirmed via commit #2046)
- Keeps the single-binary, pure-Rust deployment model

**Cons:**
- **Pins to an unreleased, moving target.** The `master` branch API can break at any commit. `TextModelBuilder` may be renamed or restructured.
- **Does not solve the systemic problem.** Next time a brand-new architecture drops (e.g., hypothetical "Claude-Open-1"), we're blocked again until `mistralrs` adds it.
- **Build times.** Building `mistralrs` from source (with `candle` + CUDA) takes 5-10 minutes on this machine.
- **Conclusion: Band-aid, not a solution.**

---

### Option 2: Replace `mistralrs` with `llama-cpp-4` (Rust bindings to llama.cpp)

**What it does:** Swap the in-process engine from Candle-based `mistralrs` to C++-based `llama.cpp` via the `llama-cpp-4` Rust crate.

**Pros:**
- **Day-0 model support.** `llama.cpp` is legendary for adding new architectures within hours of release. Gemma 4 (all variants including e4b) is fully supported.
- **Battle-tested GGUF ecosystem.** Quantized models run extremely efficiently; the community produces GGUF versions of every model within days.
- **GPU support.** CUDA, Vulkan, and Metal acceleration all work well.

**Cons:**
- **C++ build toolchain required.** CMake, Visual Studio Build Tools, and potentially CUDA SDK must be installed. This is a significant burden on Windows.
- **GGUF-only.** Cannot load raw safetensors without conversion. Limits access to models that haven't been quantized yet.
- **API surface is lower-level** than `mistralrs`. Would need to manually implement chat template rendering, tokenization, and tool-call parsing.
- **Does not support safetensors models natively.** The existing `TextModelBuilder` path for HuggingFace repos would be lost.
- **Conclusion: Better model coverage but heavy integration cost.**

---

### Option 3: Add an OpenAI-Compatible HTTP Provider (Ollama / llama-server / vLLM / Cloud)

**What it does:** Implement a new `OpenAiProvider` in `ferric-provider` that speaks the standard `/v1/chat/completions` API over HTTP to a local or remote server.

**Pros:**
- **Universal model support.** Any model that Ollama, llama-server, vLLM, TGI, or a cloud API (OpenAI, Anthropic, Google) can serve, Ferric can use. Zero architecture lock-in.
- **Day-0 support guaranteed.** Ollama confirmed to support Gemma 4 e4b with tool calling already (`ollama run gemma4:e4b`).
- **Ferric stays 100% pure Rust.** No C++ toolchains, no Python dependencies. Just `reqwest` HTTP calls.
- **Tiny code footprint.** The provider is ~150 lines: serialize messages to JSON, POST, deserialize response.
- **Enables cloud scaling.** The exact same provider works for local Ollama AND for OpenAI/Anthropic/Google cloud endpoints by changing the URL.
- **Fast compilation.** No heavy ML framework in the build graph; `reqwest` + `serde_json` compile in seconds.
- **GPU offloading handled externally.** Ollama/llama-server handle device mapping, quantization, and memory management—Ferric doesn't need to care.

**Cons:**
- **Not a single binary.** The user must install and run Ollama (or another server) separately.
- **Network latency.** localhost HTTP adds ~1-2ms per request (negligible for inference that takes seconds).
- **Dependency on external process.** If Ollama crashes or isn't running, Ferric errors out.
- **Conclusion: The most flexible and future-proof option.**

---

### Option 4: Rust-Python Bridge via PyO3 (HuggingFace Transformers FFI)

**What it does:** Embed a Python interpreter inside Ferric via `PyO3`, calling HuggingFace `transformers` directly from Rust.

**Pros:**
- **True Day-0 support.** If `pip install transformers` supports it, Ferric supports it.
- **Full ecosystem access.** `bitsandbytes`, `PEFT` LoRA, `flash-attn`, etc.
- **Precise chat template rendering** exactly as model creators intended.

**Cons:**
- **Destroys the single-binary advantage.** Users need a Python venv, PyTorch, CUDA, and `transformers` installed.
- **GIL contention.** Python's Global Interpreter Lock creates async headaches with Tokio. Every token generation pass must acquire the GIL.
- **Fragile dependency chain.** `transformers` updates can break the FFI bridge silently at runtime rather than at compile time.
- **Massive build complexity.** `PyO3` + `maturin` + Python version pinning.
- **Inference speed.** Python's per-token overhead is measurably slower than Rust/C++ native engines.
- **Conclusion: Maximum power, maximum complexity. Overkill for this use case.**

---

## Decision Matrix

| Criterion                    | Option 1 (git bump) | Option 2 (llama.cpp) | Option 3 (HTTP/Ollama) | Option 4 (PyO3) |
|------------------------------|:-------------------:|:--------------------:|:----------------------:|:---------------:|
| Day-0 model support          | ❌ Partial          | ✅ Strong            | ✅ Universal           | ✅ Universal    |
| Implementation effort        | ✅ Trivial          | ⚠️ Heavy             | ✅ Small               | ❌ Massive      |
| Single binary deployment     | ✅ Yes              | ✅ Yes               | ❌ No                  | ❌ No           |
| Pure Rust                    | ✅ Yes              | ⚠️ C++ FFI           | ✅ Yes                 | ❌ Python FFI   |
| Cloud/remote model support   | ❌ No               | ❌ No                | ✅ Yes                 | ❌ No           |
| GPU handling                 | ⚠️ Manual           | ✅ Built-in          | ✅ External            | ✅ External     |
| Build time impact            | ⚠️ Heavy            | ⚠️ Heavy             | ✅ Minimal             | ❌ Heavy        |
| LoRA/fine-tune support       | ⚠️ Limited          | ⚠️ Merged only       | ✅ Server-side         | ✅ Native       |
| Existing code compatibility  | ✅ Drop-in          | ❌ Full rewrite      | ✅ New impl of trait   | ❌ Full rewrite |

---

## Recommendation: Option 3 (OpenAI-Compatible HTTP Provider) + Option 1 (git bump) as secondary

**Primary:** Build an `OpenAiProvider` that implements the existing `Provider` trait. This is ~150 lines of Rust, uses `reqwest` + `serde_json`, and gives Ferric universal model access through any OpenAI-compatible server (Ollama, llama-server, vLLM, cloud APIs).

**Secondary:** Also bump `mistralrs` to git `master` so the embedded engine path works for Gemma 4 and we keep the single-binary option alive for users who don't want to run a separate server.

**Why this combination:** It gives Ferric two tiers of deployment:
1. **Quick/portable:** `ferric query --backend ollama --model gemma4:e4b` → talks to a running Ollama instance
2. **Standalone:** `ferric query --backend mistralrs --model-dir ./models/gguf --model-file llama.gguf` → embedded engine, no external dependencies

---

## Open Questions

1. Should the `--backend` flag be a required CLI argument, or should we auto-detect (e.g., try Ollama at localhost:11434, fall back to mistralrs)?
2. Should we keep the `mistralrs` backend as the default, or make `openai-http` the default and deprecate the embedded path over time?
3. For the git bump of mistralrs, should we pin to a specific commit hash (stable but stale) or track `master` (latest but risky)?
