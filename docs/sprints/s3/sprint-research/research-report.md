# Sprint 3 Research Report

## The Grammar Inference Problem
In Sprint 2, the `mistral.rs` engine hung when applying a strict JSON schema grammar (`llguidance`) to a small model (Llama-3.2-1B). This was root-caused to either a synthesized tokenizer mismatch or a fundamental probabilistic conflict where the model's capability floor fundamentally fights the hard grammar constraints, leading to infinite mask traversals.

## Historical Approaches
The user has tackled this in three prior iterations:
1. **[Animus (Python)](https://github.com/crussella0129/Animus)**: Used `llama-cpp-python` and scaled the grammar. Small models got strict GBNF, large models were free-form. It had a "Plan-then-Execute" loop for Nano/Small models. It was the most successful, but still brittle for the smallest models.
2. **[fev (Go)](https://github.com/crussella0129/fev)**: Relied on OpenAI-compatible APIs, offloading the grammar/JSON-schema generation entirely to the upstream server (e.g., Ollama or vLLM).
3. **[Animus_Prion (Go)](https://github.com/crussella0129/Animus_Prion)**: Used a custom "regex parser" and "LLM decomposer/chunked executor". It parsed tool calls out of free-form text instead of forcing a strict JSON AST at generation time.

## Evaluation of Approaches for Animus Ferric (Rust)
Forcing strict JSON schema through `llguidance` during generation is dangerous for small local models because if the model naturally wants to emit a token that violates the schema, the sampler forcefully masks it, sometimes leading to pathological loops or hangs.

### Alternatives:
1. **System Prompting + Regex Extraction (The Claude/Prion Way)**:
   - Provide a strong system prompt showing XML-style tool calls (e.g., `<tool>write_file</tool><path>...</path>`).
   - Let the model generate freely. If it hallucinates the format, the regex parser catches it, feeds the error back to the model ("Parse error: missing closing tag"), and lets it try again.
   - **Pros**: Will never hang the engine. Highly resilient. Works well with smaller models if few-shot examples are provided.
   - **Cons**: Costs more tokens in retries.

2. **Mini LoRA**:
   - Fine-tune a LoRA specifically for the desired JSON tool-calling format.
   - **Pros**: High accuracy.
   - **Cons**: Hard to maintain, model-specific (requires a different LoRA for Qwen vs Llama).

3. **RAG Pipeline for Tool Intention**:
   - Model outputs natural language ("I will write a file to src/main.rs"). A secondary lightweight semantic router maps this to the `write_file` tool.
   - **Pros**: Model has zero syntax burden.
   - **Cons**: Complex to build, hard to extract exact multiline arguments (like code content).

## Conclusion
The most robust approach for a local Rust harness is to drop the engine-level JSON grammar constraint and adopt a **Regex-based XML Tool Parser** coupled with a strict **Feedback-Retry Loop**. This mimics how Animus_Prion operated and aligns with how Claude parses tools, completely avoiding the `mistral.rs` sampler hang.
