# Sprint 12 Research Report — A workspace `search_files` tool

> Sprint 11 closed the mistral.rs question (ADR-027). Of the open directions,
> this research picks the one that most directly serves Ferric's *stated purpose*
> — a coding harness for **small** models — and is bounded + fully AI-verifiable.

## Decisions Reviewed
- **ADR-005** — security is hardcoded and harness-owned: a search tool must resolve every path through the `Workspace` boundary and declare its targets so the registry permission-checks them. Non-negotiable.
- **ADR-008** — all enumerated output is sorted/deterministic: the directory walk must be order-stable.
- **ADR-012 / ADR-014** — MCP-stdio is the pinned backlog (the larger alternative considered below); the capability roadmap is real work, not aspiration.
- **ADR-018** — per-tier output-token budgets: search output must be capped so a small model isn't flooded.

## The gap (why this, now)
Ferric's builtin tool surface is `read_file`, `write_file`, `list_dir`, `make_dir`, `move_path` (`crates/ferric-tools/src/builtin/`). **There is no content search.** For a *small* model that's the sharpest missing capability: without grep-style search it cannot locate a symbol/string before reading or editing — it has to guess filenames or `list_dir` blindly. Search is the foundational navigation primitive a coding agent leans on most, and the smaller the model, the more it needs to find-then-act rather than hold a whole tree in context.

## Candidates weighed
1. **`search_files` tool** — **← chosen.** Bounded (one tool, the established pattern), high-value (the core navigation gap), fully AI-verifiable (temp-workspace unit/integration tests), and security-clean (goes through the existing guard + registry chokepoint).
2. **MCP-stdio integration (ADR-012)** — high value but a large new subsystem (JSON-RPC over stdio, process lifecycle, handshake, tool adaptation, a mock server to test against). Better as its own multi-sprint effort; deferred.
3. **Live-media E2E heartbeat** — human-gated (needs a multimodal server the machine lacks). Not autonomous.

## Existing code survey (the pattern to reuse)
| File | Relevance |
|------|-----------|
| `crates/ferric-tools/src/builtin/list_dir.rs` | The closest analog — directory traversal via `ctx.workspace.resolve(path)` + sorted entries. The template for `search_files`. |
| `crates/ferric-tools/src/spec.rs` | The `Tool` trait (`spec`/`target_paths`/`run`), `ToolSpec{name,description,input_schema,permission,min_tier}`, `ToolCtx{workspace}`. |
| `crates/ferric-tools/src/builtin/mod.rs` | `register_builtin_tools` (add `SearchFiles`) + the `path_arg` helper. |
| `crates/ferric-guard/src/workspace.rs` | `resolve()` (boundary) + **`root() -> &Path`** (verified present — for `strip_prefix` to render workspace-relative match paths). |
| `crates/ferric-tools/tests/builtin_file_tools.rs` | The temp-workspace integration-test harness to extend. |

## Design (settled by the survey)
- **Substring search, dependency-free.** ferric-tools has **no `regex` dep** (verified) — a literal case-sensitive substring match keeps it ADR-004-clean and ReDoS-free. (Regex is a clean follow-on if wanted.)
- **Args:** `{query: string (required), path?: string (default "."), max_results?: number (default 50)}`. `permission: Read`, `min_tier: Nano` (a read/navigation primitive like `list_dir`). Override `target_paths` to return the search root so the registry boundary-checks it.
- **Walk:** recurse from `resolve(path)`, **entries sorted before descent** (ADR-008 determinism), skipping noise dirs (`.git`, `target`, `node_modules`, `.ferric`); read each file with `read_to_string` and **skip on error** (binary/non-UTF-8 fall away for free); collect `relpath:lineno:line` (relpath via `strip_prefix(workspace.root())`), **capped at `max_results`** (ADR-018).

## Risks / unknowns
- **Output flooding** — mitigated by the `max_results` cap + per-line trimming; the registry truncates further for the model.
- **Large trees / latency** — bounded by the result cap (stop walking once hit) and noise-dir skipping; synchronous is fine (the `Tool::run` contract is sync, T-008).
- **No security delta** — every path still resolves through `Workspace`; the tool reads only within the boundary, declares its target, and adds no new permission.

## Recommended approach
Add a `SearchFiles` builtin (substring, capped, deterministic, guard-scoped), register it, gate it at `Nano`, and cover it with temp-workspace tests (hit/miss, boundary refusal, binary-skip, cap, determinism). Document it in the tool list / README. MCP-stdio remains the next large candidate.
