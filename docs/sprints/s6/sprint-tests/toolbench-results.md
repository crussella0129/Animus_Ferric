# Sprint 6 Test Log — Toolbench Fire Rate Benchmark

**Date**: 2026-06-22
**Command**: `.\run_benchmarks.ps1 -Iterations 10`

---

## Test 1: Mistral Backend — Llama-3.2-1B-Instruct (Q4_K_M)

**Config**:
```
cargo run --release -p ferric-cli --features backend-mistralrs,backend-openai,backend-python \
  -- toolbench --backend mistral \
  --model-dir D:\Models \
  --model-file Llama-3.2-1B-Instruct-Q4_K_M.gguf \
  --iterations 10
```

**Result**: FAIL — 0.0% fire rate

```
Running toolbench across 5 tools with 10 iterations each...
Testing tool 'list_dir       ': FFFFFFFFFF [0 / 10] (0.0%)
Testing tool 'make_dir       ': FFFFFFFFFF [0 / 10] (0.0%)
Testing tool 'move_path      ': FFFFFFFFFF [0 / 10] (0.0%)
Testing tool 'read_file      ': FFFFFFFFFF [0 / 10] (0.0%)
Testing tool 'write_file     ': FFFFFFFFFF [0 / 10] (0.0%)

=== Final Fire Rate Report ===
Overall accuracy: 0.0% (0 / 50)
```

**Analysis**: The model generated tool invocations as raw text (likely XML or prose) but the MistralRS engine did not parse them into native `ToolCall` objects. The toolbench only checks `completion.message.tool_calls`, missing any text-embedded tool calls entirely.

---

## Test 2: Python Backend — Gemma-4-e4b (Safetensors)

**Config**:
```
cargo run --release -p ferric-cli --features backend-mistralrs,backend-openai,backend-python \
  -- toolbench --backend python \
  --model-dir D:\Models\google--gemma-4-e4b \
  --iterations 10
```

**Result**: CRASH — `STATUS_HEAP_CORRUPTION (0xc0000374)`

```
Running toolbench across 5 tools with 10 iterations each...
[transformers] `torch_dtype` is deprecated! Use `dtype` instead!
Loading weights: 100%|##########| 2076/2076 [00:05<00:00, 389.52it/s]
Some parameters are on the meta device because they were offloaded to the cpu and disk.
Testing tool 'list_dir       ': F
error: process didn't exit successfully (exit code: 0xc0000374, STATUS_HEAP_CORRUPTION)
```

**Analysis**: The embedded PyO3 runtime crashed after a single inference cycle. The Windows heap allocator cannot safely manage both Rust's and PyTorch's concurrent memory allocation strategies when the model is offloaded across CPU and disk. The `PythonProvider` also hardcodes `tool_calls: []`, so even a successful generation would have scored 0%.

---

## Verdict

Both backends failed to register any successful tool invocations. The failures expose three independent bugs:

1. **Missing chat template mapping** for Llama-3.2 in the MistralRS backend
2. **Heap corruption** in the embedded PyO3/PyTorch runtime under repeated inference cycles
3. **No text-to-tool-call parser** in the toolbench command itself (it only checks native tool call objects, not text output)
