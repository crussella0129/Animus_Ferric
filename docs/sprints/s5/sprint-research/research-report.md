# Sprint 5 Research Report: Native Inference for Gemma-4-e4b

## Problem Statement
We need to support Gemma-4-e4b running *natively* within Animus Ferric (without using external servers like Ollama or LM Studio). 

## Findings
1. **MistralRS limitation**: Testing the safetensors directly with the `mistralrs` backend yields an error: 
   `Unsupported Hugging Face Transformers -CausalLM model class 'Gemma4ForConditionalGeneration'. Please raise an issue.`
   This confirms `mistralrs` (v0.8.15) does not yet support the architecture of Gemma 4.
2. **llama.cpp/GGUF**: Gemma-4 is too new and not officially supported by `llama.cpp` in GGUF format yet, which is why we couldn't just drop in a GGUF file as we did with Llama 3.2.
3. **Python `transformers` passthrough**: The user previously suggested building a "rust-python passthrough". Hugging Face's Python `transformers` library natively supports new architectures like `Gemma4ForConditionalGeneration` much sooner than Rust crates or `llama.cpp`. 

## Option: Rust-Python Passthrough
To implement a native Rust-Python passthrough within Animus, we can:
- Embed Python directly into the Rust `ferric-provider` crate using `pyo3`.
- Create a Python script/module that loads the model using `transformers` and `torch`.
- Expose a `Provider` implementation in Rust (`PythonProvider`) that calls this embedded Python interpreter to perform inference.

### Pros
- **Zero-Day Support**: Any model supported by `transformers` automatically works in Animus Ferric immediately.
- **Ecosystem Access**: We gain access to `bitsandbytes` (for 4-bit/8-bit quantization), FlashAttention, and other Python-only ecosystem optimizations.
- **No External Server**: The user doesn't need to manually spin up or manage external APIs. The Python interpreter runs as part of the Animus Ferric process.

### Cons
- **Complexity and Bloat**: Requires embedding Python (`pyo3`), which complicates the build process and links against Python libraries.
- **Dependencies**: The user will need a Python environment with `torch` and `transformers` installed.
- **Performance Overhead**: Transitioning data between Rust and Python (GIL) might introduce slight overhead, although for LLM token generation, the GPU time completely dominates the GIL overhead.

## Conclusion
Given the explicit directive to run Gemma-4 natively and the impossibility of using `mistralrs` or `llama.cpp` for this specific architecture, building a `PythonProvider` using `pyo3` is the most viable path.
