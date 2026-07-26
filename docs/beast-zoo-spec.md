# Animus Beast-Zoo — brief spec (seed doc)

> **Status: not started.** This is a deliberately brief seed spec, written 2026-06-29 so it can
> be fed through a fresh agent/sprint-loop session to actually scope and build it — **as its own
> separate repo**, not a crate in Animus_Ferric. Treat everything below as a starting point for
> that loop's research phase, not a finalized design.

## One-liner

A **safetensor → GGUF customizable conversion/fine-tuning pipeline**: take a base model, apply
fine-tunes and custom pipelines, and export tuned, quantized GGUF artifacts — eventually through
a visual app, not just a CLI.

## Why it exists

[Animus_Ferric](https://github.com/crussella0129/Animus_Ferric) made a deliberate decision
(2026-06-29) to stay **GGUF-only, permanently** — cross-format model loading doesn't belong in
the agentic harness. But the *need* for safetensors↔GGUF workflows is real (the user maintains a
mixed GGUF + safetensors model library on a NAS). Beast-Zoo is where that complexity goes
instead: a **separate, dedicated tool** whose entire job is producing the GGUF files Ferric (and
other GGUF consumers) then run.

## Core capabilities (as described)

1. **Input:** a base/regular model (safetensors, presumably from Hugging Face format).
2. **Fine-tuning, two paths:**
   - **Known fine-tunes** — apply existing fine-tunes, e.g. pulled from Hugging Face
     Transformers.
   - **Custom pipelines** — user-composed pipelines: RAG, LoRA, and others (unspecified —
     "other pipelines" was left open).
3. **Output:** export to **GGUF**, at a chosen/customizable quantization.
4. **Eventually — a GUI app** with **"parametric snapping modules and sliders"**: a visual
   pipeline builder where fine-tune/quantization stages snap together like modules, each with
   sliders for its parameters (e.g. LoRA rank/alpha, quantization level/method), to produce very
   specific GGUF models.

## Relationship to the rest of Animus

- **Consumer:** Animus_Ferric (and anything else that runs GGUF) is the downstream consumer of
  Beast-Zoo's output — no direct code dependency, just an artifact hand-off (a GGUF file).
- **Not related to:** the separate, even-earlier-stage idea (recorded the same day) of eventually
  writing a **native Rust inference engine** to replace llama.cpp as Ferric's backend. That's a
  different future repo/"organ" — Beast-Zoo converts/tunes models; the inference engine *runs*
  them. No shared code expected, though both are Animus-suite components.

## Test corpus / grounding for the real design work

The reference setup has a mixed GGUF + safetensors model library on a LAN NAS, mounted as a drive,
`models` folder. A real design pass should treat this as a natural starting corpus for
scoping conversion coverage (which architectures/formats are actually present) and for
validation once a pipeline exists.

## Open questions for the real spec/research phase (not answered here)

- Language/stack: Rust-first per the user's general preference, but heavy ML tooling (fine-tuning,
  quantization) is exactly the case where the user's own rule allows falling back to Python
  (existing HF/PEFT/llama.cpp-convert ecosystage) — this needs a deliberate call, not an
  assumption.
- Which existing tools to wrap vs. reimplement: `llama.cpp`'s `convert_hf_to_gguf.py` +
  `quantize`, HF `transformers`/`peft` for fine-tuning/LoRA, vs. anything bespoke.
- What "RAG pipeline" means in a *conversion* tool context (embedding a retrieval-augmented
  dataset into a fine-tune run? something else?) — needs the user's clarification.
- Scope of the "app": local desktop GUI (Tauri/Electron, per the user's staged frontend
  preference) vs. a simpler CLI-first MVP before any visual builder.
- Repo name, license, and initial scaffolding — likely via **Animus Launch** once that exists.
