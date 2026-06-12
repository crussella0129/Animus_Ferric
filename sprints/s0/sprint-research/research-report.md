# Sprint 0 Research Report — Animus Ferric

## Decisions Reviewed

`decisions.md` is newly created and has no entries yet (this is sprint 0 of a new repo). No prior decision is being violated. The Python Animus repo's CLAUDE.md and DESIGN_PRINCIPLES.md function as de-facto prior decisions and are treated as inputs, not constraints; any we adopt will be re-recorded as ADRs in this repo's `decisions.md` during the Plan phase.

## 1. Sprint Goal

Establish the research foundation for **Animus Ferric** — a ground-up Rust rewrite, in its own repo (`C:\Users\charl\Animus_Ferric`), of the Animus local-first agentic coding harness. This sprint reviews everything the lineage has produced (Animus/Python + red-planet Rust crates, Animus_Prion/Go, fev/Go), surveys the leading open local-harness projects, and produces: (a) a refactor direction targeting efficiency, hardware/OS compatibility, deterministic scaling of task granularity to model capability ("meeting the scale" of a model), and deep testability (full replayable traces of conversation + tool calls + execution chain); and (b) a realistic verdict on 100% Rust, since a visible, demonstrable chain of ownership is itself a project requirement.

## 2. Existing Code Survey

| File | Relevance | Notes |
|------|-----------|-------|
| Animus `src/core/runtime.py` | high | The hot ReAct loop: planner-driven steps, task_complete interception, repetition guard, per-turn grammar caching. Port-faithfully list #1. |
| Animus `src/core/tiers.py` | high | 6-tier table (NANO<4B…ULTRA): grammar mode, planner budget, turns, tool count per tier. Seed of the deterministic "scale function". |
| Animus `src/core/planner.py` | high | Free-text decomposition → typed steps; `StepType.allowed_tools()` exists but unwired (known gap H2). |
| Animus `src/providers/parsing.py` | high | 5-strategy tool-call recovery from messy small-model output; provider-agnostic. |
| Animus `src/providers/native.py` | high | llama-cpp-python wrapper; tier detection from metadata/filename — known brittle (H8/H20). |
| Animus `src/observability/tracer.py` + `sinks.py` | high | TraceEvent JSONL source-of-truth + derived Rich view; full untruncated tool output in trace. The testability model to carry forward. |
| Animus `src/core/config.py` | high | Three-tier config merge; stale-config trap (H20) is the highest-impact documented bug class. |
| Animus `src/tools/defaults.py` | med | Declarative ToolSpec registration: schema + permission + min_tier. |
| Animus `SMALL_MODEL_PERFORMANCE_FINDINGS.md` | high | L0–L6 capability ladder, H1–H24 hypotheses, Llama-1B breaks at L2, 6 dominant failure modes. The empirical backbone for Ferric's design. |
| Animus `CLAUDE.md` + `docs/DESIGN_PRINCIPLES.md` | high | Real-GGUF validation policy; grammar_mode pitfalls; security-hardcoding principles. |
| Animus `crates/ferric-{cli,parse,sandbox}` (red-planet branch) | med | Existing Rust scaffold: tree-sitter parse, process sandbox ("Ornstein & Smough"), CLI. Name and intent prefigure Ferric. |
| Prion `internal/agent/agent.go` | high | Go loop: hash-ALL-calls repeat detection, context cancellation, backoff. |
| Prion `internal/planner/skeleton.go` | high | Recursive skeleton-tree decomposition + checkpoint/resume + verify-and-repair (32% speedup measured); over-decomposition guard. |
| Prion `internal/core/workspace.go` | high | Symlink-safe boundary + path-separator prefix-collision guard (documented CRITICAL fix). |
| Prion `internal/permission/checker.go` | high | Deny-lists + metacharacter/injection rejection; list-based exec only. |
| Prion `internal/llm/provider.go` + `stream.go` | med | Provider trait + hand-rolled SSE for two vendors; subprocess llama-server model (crash isolation). |
| Fev `internal/agent/loop.go` | high | Flat loop distillation — proof the lineage's essentials fit in ~150 LOC. |
| Fev `internal/memory/store.go` (+facts/hunches/corrections) | high | SQLite persistent cross-session memory with verification protocol — the lineage's best memory design. |
| Fev `internal/ctxwin/compact.go` | med | Compaction with circuit breaker (3 strikes → truncation fallback). |
| Fev `internal/tools/executor.go` | med | Single chokepoint for timeout/boundary/truncation — clean safety-layer shape. |

(Detailed per-repo surveys in artifacts: `animus-survey.md`, `prion-survey.md`, `fev-survey.md`.)

## 3. External Sources

- [llama.cpp function-calling docs](https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md) — state-of-the-art tool-call substrate: per-model templates, JSON-schema→GBNF, lazy grammars, partial-JSON healing; also documents KV-cache-quantization degrading small-model tool calling.
- [mistral.rs](https://github.com/EricLBuehler/mistral.rs) — pure-Rust inference (Candle-based): GGUF + ISQ, CUDA/Metal/CPU, integrated tool calling + grammar enforcement, llguidance merged; CUDA decode parity ±10% with llama.cpp; Windows CUDA friction documented.
- [llguidance](https://github.com/guidance-ai/llguidance) — Rust-core constrained decoding (~50µs/token; JSON Schema/regex/CFG), integrated in both mistral.rs and llama.cpp — one grammar abstraction across all backend options.
- [mini-SWE-agent + trajectories](https://github.com/SWE-agent/mini-swe-agent) — bash-only fenced-block actions, no tool-call API, >74% SWE-bench Verified; `.traj` replayable trajectories are the observability gold standard.
- [Goose docs (Ollama toolshim, logging)](https://block.github.io/goose/docs/experimental/ollama/) — the incumbent Rust harness: lead/worker model routing, SQLite sessions + OTel; notably does NOT own decoding (motivating the toolshim workaround).

(Extended source lists — Aider, OpenHands SDK, smolagents, Cline, Ollama bug trail, Candle, Burn-LM, tokenizers, crate ecosystem — in artifacts `harness-landscape.md` and `rust-feasibility.md`.)

## 4. Risks, Unknowns, Dependencies

- **Risk — mistral.rs bus factor & coverage lag:** single-lead project; new model architectures land weeks after llama.cpp. Mitigated by a pluggable inference trait with an OpenAI-compatible HTTP backend as escape valve.
- **Risk — Windows CUDA on pure-Rust stacks:** mistral.rs native Windows CUDA builds are friction-heavy (WSL2 recommended); no Vulkan. CPU-native works. Must define a supported matrix per platform/GPU vendor up front.
- **Risk — rewrite scope:** three predecessor codebases (~25k LOC combined) encode hard-won fixes (prefix-collision guard, hash-all repeat detection, circuit-breaker compaction, stale-config migration). Each must be carried as an explicit checklist item or it WILL regress; the L0–L6 benchmark ladder is the regression net.
- **Risk — grammar/termination pathology:** full-grammar mode without a structured terminator wedges small models (proven). Ferric's constrained decoding must include the `task_complete`-style escape from day one.
- **Unknown — user's GPU:** `.animus_prion/config.yaml` sets `gpu_layers: -1` (full offload), implying a usable GPU, but vendor/VRAM unverified. Determines whether mistral.rs-CUDA, CPU, or llama-server-Vulkan is the primary local path. Verify in Plan/Build.
- **Unknown — deterministic scale function shape:** mapping (params, quant, context, measured L-level) → (plan granularity, turns/step, tool count, protocol) deterministically is novel; the Animus tier table + L0–L6 data seed it, but the function needs empirical calibration sprints.
- **Dependency — crates:** mistral.rs, llguidance, tokenizers (HF), ratatui, rusqlite (bundled), tree-sitter (C core via cc — accepted), portable-pty (ConPTY quirk noted), reqwest/tokio/serde.
- **Dependency — local model inventory:** 5 GGUFs, 1B–14B, 23 GB (see `fev-survey.md`) — defines the primary design regime and the real-GGUF validation fleet.

## 5. Recommended Approach

**Primary: Animus Ferric as a Cargo-workspace Rust repo, harness-owns-decoding, dual-backend inference trait.**

1. **Inference:** a `Provider` trait with two first-class backends — **mistral.rs in-process** (flagship: pure Rust chain of ownership, direct logit access) and **OpenAI-compatible HTTP** (escape valve: llama-server for Vulkan/AMD, Ollama for convenience). Skip llama.cpp FFI bindings: they import C++ UB into the process while buying coverage the HTTP valve already provides.
2. **Constrained decoding as the harness's core competence:** llguidance grammars (JSON-Schema per tool, lazy triggers, structured terminator) driven end-to-end in the agent loop — the documented empty niche no surveyed harness fills, and exactly what 1B–14B models need.
3. **Capability ladder = deterministic scale function:** a pure function `ModelProfile { params, quant, ctx, family, measured L-level } → RunPolicy { protocol (constrained-JSON | fenced-code/bash | edit-format), plan granularity, turns/step, tool count, prompt size budget }`. This is the "sprint length vs granularity" balance made deterministic and testable — Animus's tier table generalized and calibrated by the L0–L6 benchmark harness, which moves into the Ferric repo as a first-class test substrate.
4. **Trajectory-first testability:** keep Animus's JSONL TraceEvent schema (extended: prompt assembly, grammar state, full untruncated tool output, execution-chain spans) as the source of truth; derived live TUI (ratatui); a `ferric trace replay` command for deterministic re-inspection. Real-GGUF runs gate every runtime-touching merge (lineage policy, now CI-encoded where possible).
5. **Security/memory transplants:** Prion's workspace boundary + deny-list checker and Fev's SQLite facts/hunches/corrections memory + circuit-breaker compaction ported as specified, with their documented failure modes as test cases.
6. **100% Rust verdict (honest):** achievable for everything the project owns — harness, tokenization, grammars, GGUF parsing, storage, UI. Asterisks: GPU kernels remain CUDA/Metal source compiled into the binary (inherent until Burn-LM matures), and tree-sitter's C core. The ownership chain the user wants — state and function mapped through ownership/borrowing, no Python, no FFI into mutable C++ state in-process — holds under this architecture.

**Alternative considered:** port fev 1:1 to Rust (flat loop + HTTP only) and add tiers later. Simpler first sprint, but it re-commits fev's deliberate sacrifice of small-model specialization — the lineage's actual research value and Ferric's differentiator — and HTTP-only forfeits decoder ownership, which is both the technical edge and the ownership-chain requirement. Rejected as the primary path; its loop shape is still the starting skeleton.

**Rationale:** the landscape survey shows the Rust × small-model intersection is empty and harness-owned constrained decoding is the biggest open slot; the Rust ecosystem assessment shows the parts are mature (mistral.rs + llguidance + tokenizers + ratatui); and three predecessor codebases supply battle-tested designs for every subsystem. Ferric is the synthesis: Animus's tier/grammar/trace research + Prion's security & skeleton planning + Fev's memory/compaction/UX clarity, on a Rust substrate that makes the ownership chain demonstrable.

## Artifacts

- `animus-survey.md` — full Animus (Python + red-planet Rust crates) survey.
- `prion-survey.md` — full Animus_Prion (Go) survey incl. documented failure modes.
- `fev-survey.md` — full fev survey + Corpus disposition + local GGUF inventory.
- `harness-landscape.md` — local-harness landscape (10+ projects) + gap analysis.
- `rust-feasibility.md` — Rust inference/ecosystem assessment + verdict matrix.
