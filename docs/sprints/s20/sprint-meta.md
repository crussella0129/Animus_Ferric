# Sprint 20 Meta

- **Sprint number:** 20
- **Start timestamp:** 2026-06-26T02:09:13Z
- **End timestamp:** 2026-06-26T03:00:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Validated the full multi-turn agentic loop on the real constrained backend. Wired the openai backend into the L0–L6 bench runner (additive `Invocation.openai` + pure `query_args` + `bench --backend openai/--api-base/--model`) — it was `--mock`/mistral-GGUF-only and mistral constrained hangs (ADR-027). Running it surfaced + fixed a verification bug (the `task_complete` structured terminator wasn't credited in `parse_trace`, so every spec's `expected_tools=["task_complete"]` falsely failed). Result: qwen2.5-coder:7b (ollama, ConstrainedJson) passes ALL L0–L6 → `measured_level 6`, promoting Small→Large (ADR-019 override on real data); persisted + read back by `query` (ADR-029). First end-to-end proof the constrained loop completes real tasks, not just single tool calls. ADR-030.
