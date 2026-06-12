# Artifact: mistral.rs Integration Spec (feeds s1 build directly)

> Source: web-research agent over repo master (v0.8.3), crates.io, docs.rs, docs site. 2026-06-10.

## Crate + features

- `mistralrs = "0.8.1"` (latest on crates.io, 2026-04-02; master v0.8.3 needs a git dep pinning candle to a git rev — **pin 0.8.1**). MIT, MSRV 1.88, edition 2021.
- **Default build IS the CPU build** (no accelerator features by default). Opt-in: `cuda`, `cudnn`, `flash-attn`, `metal`, `accelerate`, `mkl`. Plan: `[features] cuda = ["mistralrs/cuda"]` later.
- Windows MSVC CPU is **CI-verified upstream** on every push. Gotcha: `git config --global core.longpaths true` before building.
- aarch64: aarch64-darwin first-class; linux-aarch64 not in upstream CI — our own `cargo check --target aarch64-unknown-linux-gnu` gate covers type-level; avoid mkl/accelerate there.
- Pulls candle 0.10.2, tokenizers 0.21 (pure Rust), hf-hub, llguidance 1.2 (+lark), minijinja. Big dep tree; release binary ~30–80 MB.
- **Zero-network for local GGUF**: local model path short-circuits the HF API (`pipeline/hf.rs`); belt-and-braces `HF_HUB_OFFLINE=1` + `.with_token_source(TokenSource::None)`.

## Loading local GGUF

```rust
let model = GgufModelBuilder::new(r"C:\Users\charl\.animus\models", vec!["Llama-3.2-1B-Instruct-Q4_K_M.gguf"])
    .with_token_source(TokenSource::None)
    .with_force_cpu()
    .with_max_num_seqs(2)        // default 32 — trim for single-user
    .build().await?;
```
- Tokenizer comes from GGUF `tokenizer.ggml.*` metadata (BPE/Unigram → HF tokenizers). Chat template from GGUF `tokenizer.chat_template` (Llama-3.2 + Qwen2.5 GGUFs embed it); fallback `.with_chat_template(path)`.
- Engine runs on its own OS thread with its own multi-thread tokio runtime; harness talks over channels. Current-thread runtimes supported (warmup skip). `mistralrs::blocking::BlockingModel` exists for sync contexts.
- Load Model once, share the handle; build per-request `RequestBuilder`s.

## Requests

- Messages: `TextMessages` / `RequestBuilder::add_message(TextMessageRole::{System,User,Assistant}, text)`; tool turns via `add_message_with_tool_call` + `add_tool_message(result, call_id)`.
- Sampling: `set_sampler_temperature/topk/topp/minp/frequency_penalty/presence_penalty`, **`set_sampler_max_len`** (max_tokens), `set_deterministic_sampler()`.
- `model.send_chat_request(req).await? → ChatCompletionResponse` (OpenAI-shaped): `choices[0].message.{content, tool_calls}`, `usage.{prompt_tokens, completion_tokens, avg_compl_tok_per_sec}`.
- Streaming: `stream_chat_request` → channel-backed stream of `Response::Chunk`.

## Constrained decoding — maps 1:1 onto ferric-provider::Constraint

```rust
mistralrs::Constraint::{Regex(String), Lark(String), JsonSchema(serde_json::Value), Llguidance(TopLevelGrammar), None}
// RequestBuilder::set_constraint(...)
```
- Native tool calling: `Tool/Function {strict: Some(true)}`, `.set_tools()`, `.set_tool_choice(ToolChoice::Auto)`; per-model parsers (Qwen/Llama/etc.); strict mode grammar-enforces argument JSON mid-stream.
- **CONFLICT: a per-request Constraint applies to the ENTIRE output and fights tool-call syntax → treat `constraint: Some(_)` and non-empty `tools` as mutually exclusive per request.** (Constrained-extraction turns vs tool turns.)

## Gotchas

- 1B Q4 ≈ 0.8 GB file, ~1–2 GB RSS, seconds to load, ~20–50 tok/s CPU; 7B ≈ 4.7 GB file, ~6 GB RSS, ~4–10 tok/s.
- Threads: candle CPU uses rayon global pool → `RAYON_NUM_THREADS`.
- Don't `with_paged_attn` on CPU (GPU-oriented).
- docs.rs 0.8.1 build FAILED — read 0.7.0 docs or repo source, not "latest".
- HF cache lands on `%USERPROFILE%` — set `HF_HOME` if it matters.

## Fallback (HTTP escape valve)

llama-server `POST /v1/chat/completions`: JsonSchema → `response_format: json_schema` (server converts to GBNF); Regex/Lark have NO native mapping (would need harness-side GBNF lowering) — restrict the HTTP backend to JsonSchema-only constraints initially.

## Sources

1. https://docs.rs/mistralrs (0.7.0 docs — 0.8.1 build failed)
2. https://github.com/EricLBuehler/mistral.rs (gguf.rs, gguf_tokenizer.rs, chat_template.rs, pipeline/hf.rs, engine/, CI workflow)
3. https://crates.io/crates/mistralrs
4. https://ericlbuehler.github.io/mistral.rs/ (cargo-features, env-vars, windows install, strict-tool-calling)
5. examples: getting_started/gguf_locally, advanced/{json_schema,grammar,llguidance,tools}
