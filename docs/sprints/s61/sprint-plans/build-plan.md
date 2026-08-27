# Finalized - DO NOT EDIT

## Build Plan
1. Edit `crates/ferric-loop/src/grammar.rs` to add `"thought": { "type": "string" }` to `branch_for()`'s properties and required arrays.
2. Edit `crates/ferric-loop/src/run.rs` to update `DEFAULT_SYSTEM_PROMPT` to instruct the model to write its reasoning in the `thought` field.
