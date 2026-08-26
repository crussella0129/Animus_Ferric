Finalized - DO NOT EDIT

# Sprint 4 Test Plan: Multi-Backend Architecture

## Phase 1: Unit & Localized Testing
- Test the serialization/deserialization logic in `ferric-provider::openai::tests`:
  - Validate role mapping (System, User, Assistant, Tool).
  - Validate `ToolDescriptor` to OpenAI function calling schema mapping.
  - Validate extraction of tool calls and text from mocked OpenAI responses.
- Verify workspace builds cleanly with `cargo test --workspace`.

## Phase 2: Integration Testing
- Verify that `drive_real` correctly branches between the two providers based on CLI flags.

## Phase 3: E2E Execution & Smoke Testing
- Run `test_both_models.ps1`.
  - Validate that `ferric` successfully completes the test prompt using `mistralrs` (git bump) for Llama 3.2.
  - Validate that `ferric` successfully connects to a local Ollama instance running `gemma4:e4b` and completes the test prompt using the new `OpenAiProvider`.
