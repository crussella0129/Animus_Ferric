Finalized - DO NOT EDIT

# Native Python Passthrough (Gemma-4-e4b Support)

### `crates/ferric-provider`
We will introduce a new feature flag `backend-python` and add a new provider:

#### `Cargo.toml`
Add `pyo3` as a dependency under the `backend-python` feature.

#### `src/python.rs`
Implement `PythonProvider` struct that implements the `Provider` trait. This provider will:
- Initialize the Python GIL using `pyo3::prepare_freethreaded_python()`.
- Load a Python script (embedded or local) that handles loading the Hugging Face model using `transformers`.
- Proxy `complete()` requests to the Python function, passing messages and parameters, and parsing the returned JSON string back into a `Completion` object.

### `crates/ferric-cli`
Wire the new provider into the CLI.

#### `src/query.rs`
- Add `Python` to the `BackendArg` enum.
- Add `Python` matching logic inside `drive_real` to instantiate `PythonProvider` if `--backend python` is passed.

### `crates/ferric-provider/python`
Python scripts to be called by the `PythonProvider`.

#### `inference.py`
A python script containing a class or function that wraps `AutoModelForCausalLM` and `AutoTokenizer`, processes `transformers` chat templates, generates outputs, and returns them as structured JSON for Rust.
