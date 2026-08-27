# Sprint 7 Research: Root Cause Analysis of Toolbench Failures

## Bug 1: Llama-3.2-1B Returns 0.0% — MistralRS tool calls never fire

### Root Cause
The MistralRS provider's `complete()` method (line 136 of `mistralrs.rs`) does NOT pass tools to the engine:
```rust
// No engine-level tools or grammar constraints are passed (s3 pivot).
```

This was a deliberate decision from Sprint 3 — the s3 pivot removed engine-level constraint passing because `Constraint::JsonSchema` was hanging `mistral.rs`. The system instead relies on the XML-regex parser in `ferric-loop/grammar.rs` to extract tool calls from the model's raw text output.

**But the toolbench bypasses the loop entirely.** It sends a `CompletionRequest` with tools in the `tools` array, but:
1. The MistralRS provider **ignores the tools array** and doesn't forward them to the engine
2. The MistralRS provider returns `supports_native_tool_calls: true` in its capabilities, but this is aspirational — the actual engine invocation has tools stripped
3. The model generates tool invocations as raw XML text in `completion.message.text`
4. The toolbench only checks `completion.message.tool_calls`, which is always empty

### Fix
The toolbench must use the same XML-regex parser (`ferric-loop::grammar::parse_action`) as the main agent loop. When `completion.message.tool_calls` is empty, it should fall back to parsing `completion.message.text` using the XML regex.

Additionally, the toolbench system prompt must instruct the model to use the XML `<tool_call>` format (matching `ferric-prompt`'s protocol), not a generic "call the tool" instruction.

---

## Bug 2: Gemma-4-e4b Crashes — `STATUS_HEAP_CORRUPTION (0xc0000374)`

### Root Cause
The `PythonProvider` uses `pyo3` to embed a Python interpreter inside the Rust process. The crash sequence is:

1. `run_toolbench` creates a tokio runtime
2. For each iteration, it calls `provider.complete(request).await`
3. `PythonProvider::complete()` spawns a blocking task via `tokio::task::spawn_blocking`
4. Inside that blocking task, it acquires the GIL with `Python::with_gil`
5. Inside the GIL, it calls `inference.py::generate_completion()`
6. PyTorch allocates tensors, runs forward passes on a model offloaded across CPU/disk
7. On return, pyo3 releases the GIL and the tokio task completes
8. Repeat for next iteration

The heap corruption occurs because:
- PyTorch's CUDA/CPU allocator and Rust's allocator share the same process heap
- The `device_map="auto"` offloading strategy creates complex memory maps across disk mmap regions
- Repeated GIL acquire/release cycles from different OS threads (tokio's thread pool) create race conditions in Python's internal memory management
- Windows' heap validator detects the corruption and terminates the process

### Fix Options
**Option A: Subprocess IPC** (Recommended)
Replace the embedded PyO3 approach with a lightweight subprocess. The `PythonProvider` would:
1. Spawn a Python subprocess running a simple HTTP server (e.g. Flask/FastAPI on localhost)
2. Send inference requests via HTTP POST
3. Receive completions as JSON responses
4. The Python process owns its own heap — no cross-allocator corruption

**Option B: Single-thread PyO3**
Force all PyO3 calls onto a single dedicated OS thread (not tokio's thread pool). Use a channel-based architecture where the provider sends requests to the Python thread and awaits responses. This keeps the GIL on one thread but still risks memory fragmentation from PyTorch's allocator.

**Recommendation**: Option A is the safest long-term fix. It completely isolates the Python/PyTorch memory space from Rust's.

### Secondary Issue
Even if the crash is fixed, `inference.py` hardcodes `tool_calls: []` in its response. The Python backend currently has `supports_native_tool_calls: false`, meaning the loop already falls back to XML parsing. But the raw model output still needs the XML format — Gemma-4-e4b must be prompted with the same XML tool-call system prompt.

---

## Bug 3: Toolbench Has No Text Parser

### Root Cause
The toolbench (`toolbench_cmd.rs`) was written to test native tool-call APIs. Its success criteria (lines 98-108) is:
```rust
if completion.message.tool_calls.len() == 1 {
    let tc = &completion.message.tool_calls[0];
    if tc.name == tool.name {
        pass = true;
    }
}
```

This only works if the provider returns structured `ToolCall` objects. Neither the MistralRS backend (tools stripped from engine) nor the Python backend (hardcoded empty) ever populates this field.

### Fix
The toolbench must implement the same dual-path parsing as `ferric-loop/run.rs` (line 189-200):
1. First check `completion.message.tool_calls` (native path)
2. If empty, fall back to `ferric-loop::grammar::parse_action()` on the text output
3. The system prompt must use the XML `<tool_call>` format

This requires making `ferric-loop::grammar::parse_action` public (it already is) and adding `ferric-loop` as a dependency of `ferric-cli` (it already is).

---

## Hardware Assessment

**System**: 32 GB RAM, ~14 GB free
**Available GGUF models on D:\Models**:
| Model | Size | Suitability |
|---|---|---|
| Llama-3.2-1B-Instruct Q4_K_M | 0.75 GB | ✅ Already in use, very fast, but weak at schema |
| Phi-3-mini-4k Q4 | 2.23 GB | ✅ Good candidate — known for strong instruction following |
| qwen2.5-coder-7b Q4_K_M | 4.36 GB | ✅ Strong tool-call adherence, fits in RAM |
| c4ai-command-r7b Q4_K_M | 4.71 GB | ⚠️ Command-R has unique tool-call format |
| Qwen3-VL-8B Q4_K_M | 4.68 GB | ⚠️ Vision model, unnecessary overhead |
| Qwen2.5-Coder-14B Q4_K_M | 8.37 GB | ⚠️ Fits but tight with 14 GB free |

**Best candidate for MistralRS testing**: `qwen2.5-coder-7b-instruct-q4_k_m.gguf` — 4.36 GB fits comfortably in 14 GB free RAM, Qwen 2.5 Coder has strong instruction-following and tool-calling capabilities.

For the Python backend, Gemma-4-e4b remains the target model, but the fix must address the heap corruption first.
