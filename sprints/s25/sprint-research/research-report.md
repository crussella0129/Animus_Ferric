# Sprint 25 Research Report — Gemma 4 E4B as Ferric's reference ~4B multimodal model

> The user's reframe: the project's data shows a **~4B agentic floor**, so rather
> than a `--chat` workaround for models too small to be useful, validate a *capable*
> small multimodal model — **Gemma 4 E4B**. Research verdict: **usable, arguably
> ideal.**

## The ~4B floor (our own data)
Full-loop L0–L6 `measured_level`: **llama3.2:1b → none** (fails L0; repeat-not-terminate
+ semantic flailing, ADR-031); **llama3.1:8b → 5**; **qwen2.5-coder:7b → 6**. And
ADR-033: a 0.5B VLM (SmolVLM) garbles under the agentic scaffolding (sprint-25
re-test confirmed both `--protocol grammar` and `xml` fail — it's the agent prompt +
tools, not just the constraint). So the usable agentic floor is **~4B**, not 1B.

## Gemma 4 E4B — the fit
Released June 2026 (Google DeepMind). E4B = ~4B effective params (MatFormer/PLE).
- **Multimodal:** vision (150M encoder — OCR, charts, docs, UI, handwriting) **+ audio** (300M encoder — speech) + text.
- **Function calling** — works *inside* Ferric's constrained tool-loop (unlike SmolVLM, which can't tool-call at all).
- **128K context** — wide-context agentic runs.
- **Thinking/reasoning mode**; "byte-for-byte most capable open models"; runs on a 16 GB laptop.

## Usability — confirmed available for llama.cpp
- **Official Google GGUF + mmproj, ungated** (`gated: False`, HEAD 200): `google/gemma-4-E4B-it-qat-q4_0-gguf` → `gemma-4-E4B_q4_0-it.gguf` (5.15 GB, **QAT** q4 — best quality at this size) + `gemma-4-E4B-it-mmproj.gguf` (0.99 GB).
- Ungated community mirrors exist too (`lmstudio-community/gemma-4-E4B-it-GGUF` Q4_K_M + `mmproj-…-BF16.gguf`; `unsloth/gemma-4-E4B-it-GGUF`).
- **Edge:** a q4 4B + 1 GB mmproj ≈ 6 GB on disk / ~6 GB RAM — runs on the dev machine (it ran 8B via ollama) and is plausible on a Jetson Orin Nano (CUDA) / Pi (arm64 q4).

## Decisions Reviewed
- **ADR-031** — the 1B agentic ceiling (the floor evidence). **ADR-033** — sub-1B VLMs unusable under the loop (the caveat a capable model closes). **ADR-032** — llama.cpp is the engine; this picks the model for it. **ADR-019** — `bench` measures `measured_level` (the floor test).

## Risk
- **llama.cpp arch support:** my prebuilt `b9821` may predate Gemma 4 support (its "encoder-free multimodal" arch is novel). Mitigation: fetch a current prebuilt llama.cpp release (Google ships the GGUF *for* llama.cpp, so a recent build supports it).
- **6 GB download + ~6 GB RAM** — gated by plan approval; one-time. If it won't load even on a fresh build, the research verdict + ADR + the documented path still land; the live run defers.

## Recommended approach
Download the official GGUF + mmproj; serve via llama-server (`--mmproj`), updating
llama.cpp if needed. **Validate agentic** (`ferric bench` L0–L6 → does ~4B clear the
floor the 1B couldn't, vs 7B=6/8B=5?) **and multimodal under the constraint** (`ferric
query --file --modality image --protocol grammar` → a capable model describes the
image *inside* the agentic loop, closing ADR-033 with no harness change). ADR-035 +
docs name Gemma 4 E4B the recommended reference model. The `--chat` workaround is
dropped — a capable model removes the need.
