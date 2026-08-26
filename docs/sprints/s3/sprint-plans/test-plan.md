Finalized - DO NOT EDIT

# Sprint 3 Test Plan
1. **Parser Unit Tests**:
   - Create unit tests for `parser.rs` ensuring valid XML parses correctly.
   - Ensure malformed XML (missing tags, extra preamble, unescaped newlines) returns proper `ParseError` variants instead of panicking.
   
2. **Inference Timeout Tests**:
   - Ensure the new inference timeout correctly aborts a hanging or infinite-loop generation.

3. **E2E Simulation (Mock Provider)**:
   - Run `ferric bench` or a custom simulation using the mock provider emitting bad XML, verifying the feedback-retry loop triggers and eventually recovers (or hits max retries).

4. **Llama-3.2-1B Real Inference Test (Manual/CI Gate)**:
   - Run the E2E gate against the local GGUF model to ensure it no longer hangs at the mask traversal layer.
   - Verify it successfully executes a basic task via the XML format.
