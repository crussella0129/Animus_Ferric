# Sprint 6: Toolbench Fire Rate Framework

**Status**: failed
**Start timestamp**: 2026-06-22T14:00:00Z
**End timestamp**: 2026-06-22T22:30:00Z
**Model**: Gemini 3.1 Pro (High)
**Exit status**: failed

## Objective
Design and integrate a `ferric toolbench` CLI subcommand that isolates every registered tool call against a model, executing it multiple times to determine fire rate accuracy. Refactor backend instantiation into a shared module so both `query` and `toolbench` can reuse the same provider logic.

## Phases
- [x] Phase 1: Initialize
- [x] Phase 2: Research
- [x] Phase 3: Plan
- [x] Phase 4: Build
- [x] Phase 5: Test
- [ ] Phase 6: Loop (blocked — both backends failed)

## Failure Summary

### Finding 1: Llama-3.2-1B (MistralRS) — 0.0% Fire Rate
The Llama-3.2-1B-Instruct model scored **0/50** across all tools. The model did not produce any structured tool call objects. Root cause: the GGUF was loaded without a custom `--chat-template` mapping our XML/JSON tool schema into Llama's expected `<|python_tag|>` control tokens, and the `mistral.rs` engine's `supports_native_tool_calls` capability was not engaged. The 1B parameter model at Q4 quantization also lacks the capacity for zero-shot schema adherence without grammar constraining.

### Finding 2: Gemma-4-e4b (Python/PyO3) — Heap Corruption Crash
The Gemma-4-e4b model crashed with `STATUS_HEAP_CORRUPTION (0xc0000374)` during the first tool iteration. Root cause: the `PythonProvider` embeds the entire PyTorch runtime inside Rust's memory space via `pyo3`. Running repeated `tokio::task::spawn_blocking` cycles that re-acquire the GIL to execute `model.generate()` on a model with `device_map="auto"` (offloading to CPU/disk) corrupts the Windows heap allocator. Additionally, the `PythonProvider` hardcodes `tool_calls: []` because the `inference.py` script has no tool-call parsing logic — even if generation succeeded, it would still score 0%.

### Finding 3: Architectural Gap — No Text-to-ToolCall Parsing in Toolbench
The toolbench relies on the provider returning native `tool_calls` in the `Completion` struct. But both the MistralRS backend (with `supports_native_tool_calls: false` in practice) and the Python backend (which always returns `tool_calls: []`) produce tool instructions as **raw text**. The toolbench has no text-parsing layer equivalent to `ferric-loop`'s XML regex parser to extract tool calls from prose output. This is a fundamental design gap.
