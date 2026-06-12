# Artifact: Animus (Python) Repo Survey

> Source: Explore agent over C:\Users\charl\Animus (branch v2-polish HEAD, dirty tree; branches main, v2-rewrite, animus/red-planet inspected via git show). 2026-06-10.

## 1. Architecture map

**Runtime ReAct loop** (`src/core/runtime.py`, 560+ LOC) — `ConversationRuntime` orchestrates: planner decomposes prompt → iterate steps → each step runs inner ReAct loop until `task_complete` → accumulate. Repetition guard force-advances on duplicate `(name, args)` calls within a step. Grammar cached per-turn. Emits TURN_START → PLAN_CREATED → STEP_START/END → ITERATION_START/END → PROVIDER_RESPONSE → TOOL_CALL/RESULT → TURN_END.

**Tier system** (`src/core/tiers.py`, 80 LOC) — 6 tiers: NANO (<4B), SMALL (4–13B), MEDIUM (13–30B), LARGE (30–70B), XL (70–200B), ULTRA (>200B). Per-tier tuple: `(uses_planner, grammar_mode, max_planner_steps, max_turns_per_step, max_turns, max_tools, allows_subagents)`. NANO/SMALL use grammar_mode="full" (enabled by task_complete terminator); MEDIUM+ "off". Planner active only on NANO/SMALL.

**Planner** (`src/core/planner.py`, 100+ LOC) — free-text decomposition call (no grammar), regex-parses numbered list → `Step` objects with verb-matched `StepType`; `StepType.allowed_tools()` exists but is NOT wired to runtime (follow-on H2). Falls back to single ANALYZE step on any error.

**Provider layer** (`src/providers/base.py`, `native.py`, `mock.py`, `parsing.py`) — abstract Provider protocol; NativeProvider wraps llama-cpp-python, extracts param count from metadata or filename regex; `parse_tool_calls()` recovers tool calls from messy output (`<tool_call>` tags, ```json fences, bare/embedded JSON — 5 fallback strategies). MockProvider for CI.

**Tool registry** (`src/tools/registry.py`, `defaults.py`, 350+ LOC) — declarative ToolSpec: name, description, JSON input_schema, permission (READ/WRITE/EXECUTE), min_tier, handler. 12 default tools: fileops (move/copy/delete/make_dir, NANO), filesystem (read/write/edit/list, NANO+), search (glob/grep, NANO/SMALL), bash + git (MEDIUM+, deny-listed), task_complete meta-tool (all tiers, intercepted in runtime).

**Observability** (`src/observability/tracer.py`, `sinks.py`, 230+ LOC) — TraceEvent `{ts, session, type, data}`; dual sinks: JsonlSink (source of truth, flushed per event) + RichConsoleSink (derived). TOOL_RESULT carries FULL untruncated output; model sees truncated copy.

**Config** (`src/core/config.py`) — three-tier merge user < project < local; `validate_config_freshness()` warns on stale pre-v2.1 system_prompt (doesn't block — gap).

**Session/compaction** (`src/core/session.py`, `compactor.py`) — append-only ContentBlock messages; compaction at 70% of context, keeps recent 4 verbatim, lossy text summary of the rest.

**Security** (`src/security/permissions.py`, `workspace.py`, `deny_lists.py`) — 4 permission modes; `Workspace.resolve()` symlink-safe boundary at every tool entry; hardcoded deny lists.

## 2. Branch landscape

- **v2-polish** (HEAD, 11 ahead of main): Phases 1–8 (correctness, portability, config, usability, validation, debug mode, file-op tools + task_complete, planner wired). Uncommitted: `SMALL_MODEL_PERFORMANCE_FINDINGS.md` (52KB — L0–L6 capability ladder, H1–H24 hypotheses, Turn 4 Llama-1B baseline), `scripts/run_benchmark.py` (280 LOC harness), `tests/benchmarks/` YAML level specs.
- **v2-rewrite**: superseded foundation.
- **animus/red-planet** (29 ahead of main): Rust workspace — `ferric-cli` (104 LOC; detect/config/status in Rust, delegates heavy commands to Python subprocess), `ferric-parse` (tree-sitter AST extraction for Python/Rust/JS → JSON), `ferric-sandbox` (137 LOC; "Ornstein & Smough" process isolation: memory cap, timeout, network block, platform detection, graceful Windows degradation). NOT integrated into runtime.

## 3. Lessons encoded

- **Validate against real GGUF** before merging runtime/provider/grammar/tier changes — mocks missed 3 real bugs (CLI entry bypass; NANO grammar_mode="full" wedge pre-task_complete; tier detection collapse on missing `general.parameter_count`).
- grammar_mode="full" viable on NANO/SMALL only because of `task_complete`; never on MEDIUM+.
- Per-iteration cost multiplies on CPU (full prompt regeneration each turn; no KV-cache control from Python) — cache grammar/schemas once per turn.
- Truncate tool output before model, but trace the full output.
- `estimate_tokens()` must use real tokenizer; chars/4 over-fills small windows.
- **H20 (highest impact)**: stale pre-v2.1 user config silently breaks tier + loop terminator for every user.
- Llama-1B baseline breaks at L2; dominant failure modes: wrong tool selection (explores instead of acting), partial execution, repetition, path confusion (literal "current_workspace_directory"), stale-config trap, context pressure.
- DESIGN_PRINCIPLES.md: never timeout user input; hardcode security (LLM never makes security decisions); tool use over text; graceful degradation; ≥80% security-path coverage; explicit over implicit; local-first; fail-safe defaults; Win/macOS/Linux.

## 4. Efficiency + portability pain points

- Full prompt regeneration per turn; llama-cpp-python exposes no KV-cache reuse control.
- Token estimation noise → compaction mistimed.
- Windows: Rich UTF-8 rendering issues in default code page; resource limits unavailable (sandbox degrades); shell tool is bash-only.
- Tier detection chain (filename regex → params → tier) brittle; stale config override wins.

## 5. Testability state

- Trace JSONL: 12 event types incl. PROMPT_ASSEMBLED (full messages), PROVIDER_RESPONSE (raw + parsed + token counts), TOOL_RESULT (full output, duration). jq-queryable.
- 26 unit test files, 2600+ LOC; provider mocked everywhere — no real-GGUF in CI.
- Benchmark harness: temp workspace → run with --debug → parse trace → verify workspace state → append results.jsonl. L0–L6 specs in YAML.

## 6. Top 8 load-bearing files

1. `src/core/runtime.py` — the hot loop; every per-iteration op multiplies.
2. `src/core/tiers.py` — config cascade for all tier-aware behavior.
3. `src/core/config.py` — three-tier merge + freshness validation (H20).
4. `src/providers/native.py` + `src/providers/parsing.py` — real-GGUF interaction + 5-strategy tool-call recovery.
5. `src/tools/defaults.py` — declarative tool registration (schema + permission + tier).
6. `src/core/planner.py` — decomposition; step-scoped tools not yet wired.
7. `src/observability/tracer.py` + `sinks.py` — JSONL source-of-truth tracing.
8. `crates/ferric-cli/src/main.rs` + `delegate.rs` — existing Rust/Python boundary scaffold (red-planet).
