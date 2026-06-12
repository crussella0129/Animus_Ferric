# Artifact: Local-Harness Landscape Survey (mid-2026)

> Source: web-research agent, 2026-06-10. Full URL list at bottom.

## Projects

- **Block Goose** (Rust, Apache-2.0, ~30–45k★): ReAct core + MCP extension system (70+). Lead/worker provider routing (`GOOSE_LEAD_MODEL` plans N turns, worker executes, failure-threshold fallback) — the most explicit capability adaptation surveyed. Tool calling relies on provider-native support; experimental Ollama "toolshim" uses a second small model to translate intent→JSON. Sessions in local SQLite + OTel/OTLP export. No grammar/constrained decoding in-harness.
- **Aider** (Python, ~42–46k★): not ReAct — propose-edits/apply/auto-commit/test-feedback loop. **No tool calling at all**: per-model plain-text edit formats (whole/diff/udiff) from a model-settings registry. Tree-sitter repo map with PageRank ranking under a token budget. Architect/editor two-model mode.
- **OpenHands** (Python, MIT, ~65k★): CodeAct (actions as code) on event-sourced architecture; 2026 Agent SDK (arXiv 2511.03690). Event stream IS the trace; replayable trajectories. Recommends ~35B-class local models — not architected for 1–14B.
- **SWE-agent / mini-SWE-agent** (Python, Princeton): ACI concept — tools shaped for LLM ergonomics; YAML tool bundles + few-shot demonstration trajectories; `.traj` JSON trajectory + Inspector (gold standard observability). mini-SWE-agent: ~100 lines, ONE tool (bash) parsed from fenced text — >74% SWE-bench Verified; proof that structured tool-calling is optional.
- **smolagents** (HF, Apache-2.0): CodeAgent — actions are Python snippets in sandboxes; HF claim: code emission is a stronger small-model capability than JSON. OTel instrumentation.
- **Cline/Roo** (TS): per-model protocol switch — native function-calling OR XML-tag format, streaming parser tolerant of partial output. Weakness: 10k-token system prompt eats small contexts.
- **llama.cpp** (C/C++, ~109k★): the strongest tool-call substrate — per-model native tool-call templates, GBNF + JSON-schema→grammar, lazy grammars triggered on tool-call opening tokens, partial-JSON healing during streaming. Warning: aggressive KV-cache quantization degrades tool calling, disproportionately for small models.
- **Ollama** (Go): tools API with documented streaming tool-call delta loss (ollama#12557); tool calls parsed, not grammar-constrained.
- **OpenAI Codex CLI** (Rust): JSONL session rollouts, OS sandboxing (Seatbelt/Landlock), local models via --oss but tuned for GPT-5.x — small third-party models underperform.
- **OpenCode** (TS+Go, ~161k★ June 2026): client-server, LSP-enabled, 75+ providers.
- **Open Interpreter**: dormant since Oct 2024.

## Synthesis — state of the art for small-local-model harnesses

1. **Lower the protocol burden**: the approaches that work with weak models avoid native JSON tool calling (bash-fenced blocks, code-as-action, edit formats). JSON FC is the weakest 1–14B capability; code emission among the strongest.
2. **Constrain at the decoder when structure is needed** — llama.cpp grammars make malformed calls impossible; yet NO harness drives this end-to-end (all sit behind HTTP and hope).
3. **Per-model protocol switching** (Cline XML-vs-native, aider edit-format registry, Goose lead/worker) is scattered, nowhere unified.
4. **Harness-side determinism compensates for model weakness**: test/lint feedback loops, repo-map token budgeting, edit-repair retries, ACI-shaped tools, few-shot trajectories.
5. **Observability convergence**: local SQLite/JSONL session store, replayable trajectories, OTel export.
6. **Small contexts are the silent killer**: huge system prompts + 4k default contexts wreck small-model tool calling.

## Gaps Animus Ferric can fill

- No Rust harness purpose-built for 1B–14B. The Rust × small-model intersection is EMPTY.
- Grammar-integrated agent loop (harness owns sampling: per-tool JSON-schema grammars, lazy triggers, KV-quant guardrails) — the single biggest open architectural slot.
- A unified capability ladder selecting protocol per model: constrained-JSON → fenced code/bash → edit formats; shrink tool count + prompt accordingly.
- Single-binary harness+inference (kills the Ollama streaming-delta bug class).
- Trajectory-first observability (.traj-grade replay + SQLite sessions + OTel) from day one.
- Windows-native sandboxing (AppContainer/job objects) — nobody has it.
- Verification loops (compile/test/lint gates, JSON healing) as first-class state machine stages.

## Sources (5 most authoritative first)

1. https://block.github.io/goose/docs/experimental/ollama/ + https://block.github.io/goose/docs/guides/logs/
2. https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md
3. https://github.com/SWE-agent/mini-swe-agent + https://swe-agent.com/latest/usage/trajectories/
4. https://arxiv.org/html/2511.03690v2 + https://docs.openhands.dev/openhands/usage/llms/local-llms
5. https://aider.chat/docs/repomap.html + https://aider.chat/docs/faq.html

Extras: huggingface/smolagents; deepwiki.com/cline/cline/4.6-system-prompts-and-tool-definitions; docs.ollama.com/capabilities/tool-calling; github.com/ollama/ollama/issues/12557; github.com/block/goose/issues/4036; deepwiki.com/sst/opencode; github.com/charmbracelet/crush/discussions/1828; changes.openinterpreter.com/log/local-iii; arxiv.org/pdf/2405.15793 (ACI); ollama.com/blog/codex.
