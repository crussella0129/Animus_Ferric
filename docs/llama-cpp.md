# Running Ferric on llama.cpp (`llama-server`)

llama.cpp's `llama-server` is Ferric's **recommended engine** (ADR-032). It's the
OpenAI-compatible HTTP valve that enforces the constrained grammar — the same path
the whole harness is built on — but with **raw tok/s**, a **context window as wide
as you want**, the **multimodal** path (`--mmproj`), and a **single minimal binary**
that runs on the edge (Jetson Orin Nano, Raspberry Pi + AI hat). Ollama still works
as a one-flag fallback (`--engine ollama`), but llama.cpp is leaner and gives you
full control.

> Validated live (sprint 23): Ferric drives `llama-server` under the constrained
> protocol at **100% Ring-0 tool-call fire rate — identical to ollama**.

## 1. Get `llama-server`
Grab a prebuilt release (no build needed) from
<https://github.com/ggml-org/llama.cpp/releases> — pick the asset for your hardware:
- **CPU:** `llama-<b>-bin-win-cpu-x64.zip` (or `-arm64` for Pi/Jetson aarch64).
- **NVIDIA (Jetson, desktop):** `…-bin-win-cuda-*.zip` (+ the matching `cudart-*` zip).
- **Vulkan / HIP / SYCL / OpenVINO** builds are also published.

Unzip; `llama-server` + the `ggml-*` libraries sit together (keep them in one dir).

## 2. Point it at a model — reuse an ollama blob (no re-download)
If you already `ollama pull`-ed a model, its weights are a **plain GGUF** in
ollama's blob store, so you don't need to download anything again:

```sh
# Resolve the GGUF blob for a tag (the `image.model` layer):
#   ~/.ollama/models/manifests/registry.ollama.ai/library/<model>/<tag>
#   → the layer with mediaType .../image.model → ~/.ollama/models/blobs/sha256-<digest>
llama-server -m ~/.ollama/models/blobs/sha256-<digest> -c 8192 --host 127.0.0.1 --port 8080
```

Or use any `.gguf` you downloaded directly. (Ferric's launcher does this for you —
see §4.)

## 3. Wide context for agentic runs
ollama defaults to a narrow context (`num_ctx` 2048/4096 unless tuned per model).
llama-server's `-c` goes **as wide as VRAM/RAM allows**:

```sh
llama-server -m model.gguf -c 16384 ...   # or -c 0 = the model's full trained context
```

A wider context means more tool results + history fit before truncation — better
multi-turn/agentic results. (Ferric's per-turn prompt budget still derives from the
model *profile* `--ctx`; keep them consistent.)

## 4. Drive it with Ferric
Two ways:

```sh
# (a) let Ferric launch + manage it (writes .ferric/server.json so other commands auto-discover it):
ferric server up --engine llama-server --model /path/to/model.gguf [--mmproj mmproj.gguf] [--ctx 8192]
ferric query  --backend openai --protocol grammar "…your task…"     # auto-discovers the server
ferric server down

# (b) or point Ferric at a server you started yourself:
ferric query --backend openai --api-base http://127.0.0.1:8080/v1 --model any-label --protocol grammar "…"
```

`--engine ollama` remains available if you prefer ollama for model management.

## 5. Multimodal (image / audio / video)
llama-server + a projector GGUF (`--mmproj`) is the media path. **Validated live**
(sprint 23–24): an image routed by Ferric reaches the vision encoder and the model
describes it correctly. You supply a vision model GGUF **and its mmproj** (ollama's
models are text-only, so there's no blob to reuse for vision):

```sh
# A tiny vision model to try — SmolVLM-500M (model ~436MB + mmproj ~108MB):
#   huggingface.co/ggml-org/SmolVLM-500M-Instruct-GGUF
llama-server -m SmolVLM-500M-Instruct-Q8_0.gguf --mmproj mmproj-SmolVLM-500M-Instruct-Q8_0.gguf -c 4096 --port 8080

ferric query --backend openai --api-base http://127.0.0.1:8080/v1 \
  --file pic.png --modality image "what is in this image?"
```

Ferric base64's the file into an OpenAI `image_url` content-part (`data:<mime>;base64,…`)
that llama-server's mmproj consumes — proven against real pixels (SmolVLM answered
"Red." to a red square). **But a sub-1B VLM degrades under the JSON tool-call grammar**
(SmolVLM-500M garbled inside the agentic loop). Use a *capable* model instead.

### Recommended model — Gemma 4 E4B (the reference, ADR-035)
**Gemma 4 E4B** is Ferric's recommended model: ~4B effective, **multimodal (vision +
audio)**, **function-calling**, 128K context, edge-feasible. The data shows ~4B is the
usable agentic floor (a 1B completes nothing; Gemma 4 E4B reaches **`measured_level 5`**,
matching an 8B), and it **describes images *inside* the constrained agentic loop** —
no workaround needed. Official, ungated GGUF + mmproj:

```sh
# google/gemma-4-E4B-it-qat-q4_0-gguf  (model 5.15GB QAT-q4 + mmproj 0.99GB)
llama-server -m gemma-4-E4B_q4_0-it.gguf --mmproj gemma-4-E4B-it-mmproj.gguf -c 8192 --port 8080
ferric query --backend openai --api-base http://127.0.0.1:8080/v1 \
  --file pic.png --modality image "describe this image, then call task_complete"
```

> **Speed:** use a **CUDA/Vulkan/Metal** llama.cpp build for usable latency — a 4B on a
> CPU build runs at tens of tok/s and timed out the simplest bench level. On a GPU
> (incl. Jetson Orin Nano) it's fast.

See [docs/multimodal.md](multimodal.md) for `--file`/`--modality` routing; ADR-033/035 for the validation.

## 6. Edge notes (Jetson Orin Nano / Raspberry Pi)
- llama.cpp is the edge inference engine: one static binary + ggml libs, no daemon.
- **Jetson Orin Nano:** use a CUDA build; the Orin's GPU runs small models fast.
- **Raspberry Pi (+ AI hat):** use the `arm64` CPU build (or the accelerator's
  backend if supported); pick a small quant (Q4_K_M) of a 1–3B model.
- Keep the model small and the binary local — Ferric itself is a tiny Rust CLI, so
  the whole stack stays minimal. The constrained valve means even a 1B fires tool
  calls at 100% (it just isn't a multi-turn *agent* — ADR-031).
