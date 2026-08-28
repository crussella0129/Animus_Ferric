# Sprint 114 Model Selection

## Decision

Select the pinned third-party Unsloth conversion of the official upstream
`Qwen/Qwen3.8-27B`, exact primary artifact
`Qwen3.8-27B-UD-Q4_K_M.gguf`, and save it as
`models/Qwen3.8-27B-UD-Q4_K_M.gguf`.

- Upstream model license: Apache-2.0
- GGUF repository revision:
  `313447f257f7ebde0b968e4778feef774546ed81`
- Exact size: `16,464,440,224` bytes (about 15.33 GiB)
- Converter-published SHA-256:
  `322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482`
- Pinned artifact:
  `https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/resolve/313447f257f7ebde0b968e4778feef774546ed81/Qwen3.8-27B-UD-Q4_K_M.gguf?download=true`
- Official model card: <https://huggingface.co/Qwen/Qwen3.8-27B>
- Pinned GGUF tree:
  <https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/tree/313447f257f7ebde0b968e4778feef774546ed81>

Unsloth is the conversion publisher, not the official Qwen model publisher.
The initial Qwen3.5-9B Q5_K_M recommendation was superseded before plan freeze
after the user asked for Qwen3.8 specifically. This record preserves that
decision change instead of presenting the later selection as the original one.

## Why this quantization

Qwen3.8 currently offers one locally plausible official dense model size:
27B. Its published coding and agentic results make it the best capability
hypothesis found for this task, but those full-precision publisher results are
not local quantized Ferric results. Q4_K_M is the highest-quality practical
quant on the measured 31.93 GiB RAM / 11 GiB VRAM host. It cannot fit wholly in
VRAM, so the plan uses measured CPU/GPU hybrid offload rather than implying
full-GPU inference.

The architecture has 64 language layers arranged as 16 groups with one full
attention block per group, four KV heads, and 256-dimensional attention heads.
From those published dimensions, an f16 KV cache at context `32768` is
approximately 2 GiB; Q8 K/V is approximately 1 GiB before runtime buffers. The
initial coordinate therefore uses context `32768`, 24 GPU layers, Q8 K/V
cache, one slot, and flash
attention. At plan freeze about 15.04 GiB system RAM and 8.57 GiB VRAM were
free, so compatibility and throughput remain measured gates rather than fit
claims. Startup rechecks availability and does not assume those bytes remain
free.

Qwen reports 262,144 native context and up to 1M extended context. The local
`32768` coordinate is deliberately much smaller and must be described as the
usable tested window, not the trained window. The publisher reports 73.0 on
TerminalBench 2.1, 61.7 on SWE-bench Pro, 42.3 on NL2Repo, 42.2 on DeepSWE,
79.0 on QwenSWEBench, 70.7 on CoWorkBench, and 90.3 on LiveCodeBench v6. Most
coding-harness results used 256K context and temperature 1.0; none may be
attributed to this GGUF until measured locally.

## Frozen fallback rule

The primary Q4 coordinate is operationally viable when it loads at context
`32768` or the single declared `16384` memory fallback, completes the nonce
grammar smoke without mutation or clarification, and reaches a median of at
least 2.0 decoded tokens per second. Throughput uses one unscored warm-up and
exactly three identical 256-token timed requests with no replacement samples.
Any request error, timeout, or result below 128 decoded tokens makes the
coordinate non-viable because no valid three-sample median exists.

Only if Q4 fails that rule may acquisition fetch the pinned
`Qwen3.8-27B-UD-Q3_K_XL.gguf` fallback:

- Exact size: `13,146,393,504` bytes (about 12.24 GiB)
- Converter-published SHA-256:
  `8c2a45ff85e7674ca185ec8eb6cdeab0e617ed9d8018caed0b64380eb2a67a5e`
- Save path: `models/Qwen3.8-27B-UD-Q3_K_XL.gguf`
- Pinned artifact:
  `https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/resolve/313447f257f7ebde0b968e4778feef774546ed81/Qwen3.8-27B-UD-Q3_K_XL.gguf?download=true`

Q3 receives the same prompt, context, cache, sampling, and measurement method,
with exactly 32 requested GPU layers. It is tested only after Q4 is non-viable
and replaces Q4 only when it also completes the functional smoke and its
three-sample median reaches at least 2.0 tokens per second. Otherwise the
sprint records that no viable Qwen3.8 coordinate was found. The different
fixed offload counts are intentionally an end-to-end comparison of two
predeclared host coordinates, not a quantization-only benchmark.

## Alternatives

1. Qwen3.8 2-bit/IQ2 GGUFs are smaller, but the expected precision loss is a
   poor match for exact grammar/tool syntax and compounding multi-turn coding
   decisions. They are rejected rather than treated as automatic fit wins.
2. Qwen3.5-9B Q5_K_M is outside the frozen ladder. Acquiring or using it would
   require a new retained plan deviation; it is not a silent compatibility
   fallback.
3. Existing Qwen2.5-Coder-7B Q4_K_M is the sole frozen fallback simulation and
   skill-audit control if neither Qwen3.8 coordinate is viable. It is proven to
   run through Ferric, but Sprint 113 recorded 0/3 completion on its frozen
   long-horizon screen, so its result is never presented as Qwen3.8 evidence.
4. Qwen3-Coder-Next is attractive for agentic coding but its resident GGUF is
   impractical on this host despite sparse active parameters.

## Runtime caveats

- The installed llama.cpp build is recent, but Qwen3.8 architecture, chat
  template, thinking output, and Ferric's constrained route remain unproved
  until the smoke.
- Qwen3.8 thinks by default. The publisher recommends thinking-mode
  `temperature=1.0`, `top_p=0.95`, and `top_k=20`; Ferric and llama.cpp expose
  different subsets. The run records every effective setting and does not
  claim an exact publisher coordinate.
- The managed child inherits llama.cpp environment settings for Q8 K/V cache,
  flash attention, bounded thinking, a device/VRAM fit margin, HTTP timeout,
  and thinking preservation.
  Startup logs, endpoint identity, throughput, and memory snapshots—not the
  planned flags—are authoritative.
- The optional vision projector and MTP head are not needed for this text-only
  trial and will not be downloaded.
