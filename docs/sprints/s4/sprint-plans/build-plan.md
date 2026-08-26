Finalized - DO NOT EDIT

# Sprint 4 Build Plan: Multi-Backend Architecture

## 1. ferric-provider: Add OpenAiProvider
- Create `crates/ferric-provider/src/openai.rs`.
- Implement `OpenAiConfig` and `OpenAiProvider` mapping Ferric `Message` and `ToolDescriptor` to the OpenAI `/v1/chat/completions` schema using `reqwest`.
- Add `backend-openai` feature flag in `crates/ferric-provider/Cargo.toml` enabling `reqwest`.
- Update `crates/ferric-provider/src/lib.rs` to expose `openai` under the feature flag.

## 2. Workspace Setup
- Add `reqwest` to workspace `Cargo.toml`.
- Update `mistralrs` dependency in `Cargo.toml` from `=0.8.1` to the git repository `master` branch.

## 3. ferric-cli: Plumb the Backend Flag
- Update `crates/ferric-cli/Cargo.toml` to forward the `backend-openai` feature.
- Modify `crates/ferric-cli/src/query.rs`:
  - Add `--backend` flag taking `Mistral` or `Openai` (default: `Mistral`).
  - Add `--api-base` and `--api-key` arguments.
  - Add `--model` argument for the OpenAI backend.
  - Refactor `drive_real` to construct and use the correct provider based on the backend argument.

## 4. Test Infrastructure
- Update `test_both_models.ps1`:
  - Run the Llama model using `--backend mistral --model-dir ... --model-file ...`
  - Run the Gemma model using `--backend openai --model gemma4:e4b`
