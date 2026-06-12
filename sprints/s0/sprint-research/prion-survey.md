# Artifact: Animus_Prion (Go) Repo Survey

> Source: Explore agent over C:\Users\charl\Animus_Prion + C:\Users\charl\.animus_prion. 2026-06-10.

## What it is

100% Go 1.26 rewrite of Animus (no CGo; modernc.org/sqlite; cobra CLI). Same architecture & security model as Python Animus; different LLM interface — HTTP to a llama-server subprocess instead of in-process bindings. v0.2.0, 16.7 MB static binary, 170 passing tests across 27 test files, 20 linear commits on `main` only (2026-03-17 → 2026-03-23). CI: ubuntu + windows (go test/vet/build/gofmt). Production-ready snapshot, no longer the active line.

## Architecture (13 packages, 62 files, ~13k LOC)

- `cmd/prion` — cobra CLI: REPL, run, config, setup (502 LOC).
- `internal/agent/agent.go` — agent loop: tool calling, reflection, repeat detection (hashes ALL tool calls), streaming, context.Context cancellation.
- `internal/core` — workspace boundary (symlink-safe + path-separator guard against prefix collision), tier-aware token budgets, error classification, unified `core.Message` type, 4-strategy JSON tool-call extraction.
- `internal/llm` — Provider interface; OpenAI-compatible + Anthropic clients; subprocess llama-server native provider; SSE streaming hand-parsed with bufio; GPU detection (nvidia-smi → rocm-smi → CPU); GBNF grammar constraints.
- `internal/permission` — deny-lists (dirs/files/commands/network), metacharacter rejection, injection regex.
- `internal/tools` — registry with schema validation; list-based exec (no shell interpolation); filesystem with audit log; 8 git ops; manifold search tool.
- `internal/planner` — **skeleton tree decomposition**: recursive TaskNode tree, leaf heuristics, maxDepth=3, session checkpoint/resume, verify-and-repair loop (auto-compile after write; R2 benchmark: 32% speedup).
- `internal/retrieval` — **100% hardcoded query router** (<1ms, deterministic, injection-proof) + RRF fusion (k=60).
- `internal/knowledge` — Go AST parser + regex multi-lang; SQLite graph DB with BFS (callers/callees/blast radius); incremental SHA-256 indexing.
- `internal/memory` — SQLite vector store, cosine KNN + pure-Go HNSW (~100k chunks).

## Failure modes documented (carry forward)

1. Workspace prefix collision (`"project-evil"` passes `HasPrefix("project")`) — fixed with path-separator guard. CRITICAL.
2. shell=true injection — use list-based exec only.
3. Unbounded HTTP reads → OOM — io.LimitReader everywhere.
4. Tool args must be polymorphic (string/array/object) — model output varies.
5. Repeat detection must hash ALL tool calls, not just first.
6. Exponential backoff on retryable errors.
7. TrimMessages proactively; edge case when system prompt > maxTokens.
8. Sort all enumerated outputs (map iteration nondeterminism kills reproducibility).

## Proven ideas

Unified message type; context propagation/cancellation everywhere; hardcoded retrieval router beats LLM routing; skeleton trees > flat plan lists for complex tasks (with over-decomposition guard); GBNF for tool calls works at 7B; verify-and-repair loop; hardware auto-detect fallback chain; pure-SQLite (no C deps) worth +15MB binary; subprocess inference = crash isolation + portability.

## Top 5 load-bearing files

1. `internal/agent/agent.go` — the loop.
2. `internal/planner/skeleton.go` — recursive decomposition + checkpoint/resume.
3. `internal/core/workspace.go` — security boundary.
4. `internal/permission/checker.go` — deny-lists + injection detection.
5. `internal/llm/provider.go` + `stream.go` — provider abstraction + SSE.

## ~/.animus_prion/config.yaml

native provider, Qwen2.5-Coder-14B Q4_K_M, ctx 4096, gpu_layers -1 (full GPU offload), size_tier medium, max_turns 20.
