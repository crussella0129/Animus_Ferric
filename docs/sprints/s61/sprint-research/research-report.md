# Research Report: Constrained Decoding Code Quality Degradation

## Findings
When using `ferric chat` conversationally, the Qwen model generates high-quality Python code (iterative Fibonacci with docstrings and type checking) because it runs in an unconstrained environment, allowing it to output markdown and use Chain-of-Thought (CoT) reasoning before it outputs code.

When using `/do` (or `ferric query`), the agent transitions to the `Nano` tier, which utilizes the `ConstrainedJson` ActionProtocol. 
- In `crates/ferric-loop/src/grammar.rs`, the `action_schema` enforces strict JSON schema (`additionalProperties: false`) with only `"tool"` and `"args"` fields.
- Because the model is strictly constrained to output `{"tool": "write_file", "args": {...}}` from the very first token, it is completely deprived of the ability to output markdown or "think" (Chain of Thought).
- As a result, when it is forced into a zero-shot code generation task inside a JSON string, it defaults to the simplest/shortest valid code sequence (the recursive one-liner) to satisfy the constraint with the highest probability, leading to severely degraded coding performance compared to its conversational mode.

## Proposed Solution
We can introduce a Chain-of-Thought vector directly into the `ConstrainedJson` schema.
1. Modify `branch_for` in `crates/ferric-loop/src/grammar.rs` to include a `"thought": { "type": "string" }` property, and add it to the `"required"` array.
2. Modify the `DEFAULT_SYSTEM_PROMPT` in `crates/ferric-loop/src/run.rs` to instruct the model to think in the `"thought"` field before executing a tool.

This allows the model to "think out loud" (perform CoT) *inside* the constrained JSON envelope before it generates the `args` payload, significantly boosting code generation quality while maintaining 100% tool reliability (since the schema is still strictly enforced by the backend).
