Finalized - DO NOT EDIT

# Sprint 3 Build Plan
1. **Remove `llguidance` and strict grammar from Inference Engine**:
   - Strip `serde_json` strict structures from prompt builders.
   - Remove grammar constraints from `mistral.rs` pipeline in `ferric-core/src/inference.rs`.
   - Add hard per-request inference timeout and wall-clock kill for safety.

2. **Refactor System Prompt for XML Tool-Calling**:
   - Update `ferric-prompt/src/system_prompt.rs` (or equivalent) to remove JSON schema instructions.
   - Inject XML syntax templates (`<thought>`, `<tool_call>`, `<name>`, `<args>`, etc.).
   - Introduce few-shot examples for Small Tier models to ensure syntax compliance.

3. **Implement XML/Regex Parser**:
   - Modify `ferric-loop/src/parser.rs`.
   - Implement resilient regex-based extraction of `<thought>` and `<tool_call>` elements.
   - Map extracted XML data back to the existing `Action` and `ActionProtocol` enums.
   - Return descriptive `ParseError` types when tags are malformed or missing.

4. **Implement Feedback-Retry Loop**:
   - Modify `ferric-loop/src/executor.rs`.
   - Catch `ParseError` from the parser.
   - Generate a User/System message detailing the parse error ("Missing closing tag", etc.).
   - Resubmit to the model, bounded by `MAX_RETRIES` (e.g., 3).
