Finalized - DO NOT EDIT

# Verification Plan

### Automated Tests
- Create unit tests in Rust to ensure the `PythonProvider` formats inputs and parses outputs correctly by mocking the Python call.

### Manual Verification
- Expand `test_both_models.ps1` to run the Gemma-4-e4b model using the new `--backend python` argument. Verify that the Python backend successfully loads the model and executes tool calls correctly.
