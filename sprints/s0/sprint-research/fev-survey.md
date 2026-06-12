# Artifact: Fev (Go) Repo Survey + Corpus + Local Models

> Source: Explore agent over C:\Users\charl\Fev (github.com/crussella0129/fev), C:\Users\charl\Corpus, C:\Users\charl\.animus\models. 2026-06-10.

## What fev is

Go 1.26 local-first agentic CLI harness, v0.3.0, ~7.8k LOC, 82 files, the most recent lineage iteration. Radical direction shift vs Animus: drops tier system + GBNF + planner in favor of a **flat agent loop**, **OpenAI-compatible HTTP only** (Ollama/llama-server/vLLM/LM Studio), **SQLite persistent memory** (facts/hunches/corrections with verification protocol + post-session review), **bubbletea TUI** (lipgloss + glamour markdown), ecosystem compatibility (reads CLAUDE.md, MCP-aware, Claude Code tool names).

## Architecture

- `internal/agent/loop.go` — flat ReAct: generate → parse tool calls → dedup → repeat detection (3× threshold) → execute → loop.
- `internal/ctxwin/` — compaction with **circuit breaker** (3 strikes → fall back to truncation); tool-pair invariant preservation.
- `internal/memory/` (1194 LOC) — SQLite (modernc, no CGo); facts/hunches/corrections tables with source tracking; memory-as-hint verification.
- `internal/tools/` — 10 core tools (read/write/edit/bash/grep/glob/list_dir/git), executor with timeout + workspace boundary + output truncation.
- `internal/llm/` — OpenAI-compatible client, SSE streaming, token counting.
- `internal/permission/` — deny-lists + injection detection.
- `internal/ui/` — bubbletea state machine, spinner verbs, diff rendering.

## Branches

`main` = v0.2.0 (loop + memory); `feature/plan-3-ui` (HEAD, +8, tag v0.3.0) = bubbletea UI; `feature/plan-2-memory-compaction` merged into main. Active CI.

## What fev gained / lost

Gained: simplicity, portability, polished TUI, persistent cross-session memory, ecosystem interop. Lost (deliberately): tier-adaptive grammar enforcement, parameter-count tier detection, multi-step planner — i.e. the small-model specialization that is Animus's actual research value.

## Corpus

Unrelated: Electron+React+TS spaced-repetition flashcard app (FSRS scheduler, Cytoscape graph). Not part of the lineage; ignore for Ferric.

## Local GGUF inventory (C:\Users\charl\.animus\models, 23 GB)

| Model | Size | Tier |
|---|---|---|
| Llama-3.2-1B-Instruct Q4_K_M | 771 MB | NANO |
| Qwen2.5-Coder-7B-Instruct Q4_K_M | 4.4 GB | SMALL (primary) |
| c4ai-command-r7b abliterated Q4_K_M | 4.8 GB | SMALL (parser-stressing) |
| Qwen3-VL-8B abliterated Q4_K_M | 4.7 GB | SMALL (VL outlier) |
| Qwen2.5-Coder-14B-Instruct Q4_K_M | 8.4 GB | MEDIUM |

Ferric's tier design should target this 1B–14B band as the primary regime.

## Top 5 load-bearing files

1. `internal/agent/loop.go` — the engine.
2. `internal/memory/store.go` + facts/hunches/corrections — persistent memory differentiator.
3. `internal/ctxwin/compact.go` — circuit-breaker compaction.
4. `cmd/fev/main.go` — integration hub.
5. `internal/tools/executor.go` — safety layer all calls flow through.
