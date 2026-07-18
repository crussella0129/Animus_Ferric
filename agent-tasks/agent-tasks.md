# Refactoring Tasks (From Architecture Report)

## animus-launch
- [ ] Migrate `std::fs` to `tokio::fs` for async filesystem operations in project scaffolding.

## ferric-bench
- [ ] Decouple Validation from Traces in `src/verify.rs` by introducing an `Assertion` trait.

## ferric-cli
- [ ] Consolidate configuration precedence by merging `config.rs` and `backend.rs` into a unified `ConfigManager`.

## ferric-core
- [x] Isolate base64 encoding from `media.rs` into an optional module or crate feature (`media-encoding`).
- [x] Standardize errors in `error.rs` to derive `thiserror::Error`.

## ferric-guard
- [x] Implement Dynamic Denylist Configuration in `src/denylist.rs` to read from a `.ferricignore` file. *(Sprint 77, ADR-068 — `IgnoreList` (`ignore.rs`) parses a gitignore-flavored `.ferricignore` from the workspace root; `check_with_ignore` folds it into the registry chokepoint as **additive-only** denials (never relaxes the hardcoded ADR-005 floor). The policy file is itself write-denied.)*

## ferric-loop
- [x] Extract Loop State from `run.rs` into a dedicated `LoopState` struct.
- [x] Decouple Driver from Logic by breaking down the monolithic `while` loop into a `step(&mut LoopState) -> Result<TurnOutcome>` function.

## ferric-prompt
- [x] Implement a templating engine to replace manual `format!()` strings in `src/lib.rs`. *(Done: superseded by `oovra` element composition, ADR-016 — `recipe_for`/`compose` render prompt atoms, not `format!()`.)*

## Advanced Harness Features (Plan, Stop, Revert, Dream)
- [x] **Plan Mode**: Implement an `ActionProtocol::Plan` that strips write-permissions and provides a `submit_plan` terminator, effectively utilizing the robust constraint and tool-call mechanics to safely enforce an initial planning phase.
- [x] **Graceful Interrupts (Stop):** Integrate `tokio::signal::ctrl_c` handling in the CLI driver and pass a cancellation token to `LoopState` to abort execution and gracefully commit a `SessionEnd` trace.
- [x] **Time Travel (Revert):** Implement a lightweight VCS wrapper (`ferric-vcs`) to automatically orphan/stash workspace states tied to trace `TurnEnd` events, allowing the CLI `revert <turn_id>` command to rollback both the workspace and the replay trace.
- [x] **Dream Mode:** Create an asynchronous offline `ferric dream` worker that parses historical `.ferric/traces`, extracts high-value signals/patterns, and consolidates them into a persistent memory context (`MEMORY.md` or `.ferric/knowledge/`).
- [x] **Configurable Hooks:** Introduce a `ferric.toml` or `hooks/` system for synchronous pre-turn, post-turn, or on-error scripts.
- [x] **Background Task Management:** Add detached `tokio` process management to `shell_exec` and expose a `manage_task` built-in tool.
- [x] **Agent Delegation Structure (ICM):** Build a `ferric-icm` orchestrator crate that orchestrates sub-agents entirely via sequential local folders (`01_research/`, `02_script/`) and their `CONTEXT.md` files. *(Sprint 73 ADR-064 — inc 1: `ferric-icm` crate + `ferric icm init`/`plan`. Sprint 74 ADR-065 — inc 2: `ferric icm run` executes each stage through the constrained loop, contained to its own folder, with halt-on-failure + human review gates (`--auto`/`--from`/`--to`/`--mock`). See `docs/icm.md`. Follow-ups: Ornstein-quarantined web-research stage, workspace-builder.)*
- [ ] **Interactive "Accept Edits" Mode:** Pause the driver to display unified diffs via a `PendingEdit` state, awaiting user confirmation before flushing to disk.
- [ ] **Direct Terminal Passthrough:** Allow commands prefixed with `!` or `/run` in the chat UI to execute instantly via `shell_exec` without LLM roundtripping.
- [x] **Agentic Cron Jobs:** Introduce a `.ferric/cron/` directory and background watcher to schedule periodic agent tasks (e.g., `/dream every 12 hours`). *(Sprint 75, ADR-066 — `ferric-cron` crate (schedule/due/state, pure) + `ferric cron add`/`list`/`run`/`watch`. Jobs run a bounded set of Ferric subcommands (`dream`/`query`), never arbitrary shell. See `docs/cron.md`. Deferred: crontab expressions, detached daemon w/ runfile.)*

## ferric-provider
- [ ] Optimize SSE streaming in `src/stream_scan.rs` using `bytes::BytesMut` and `serde_json::StreamDeserializer` instead of `String` buffering.

## ferric-research
- [x] Define a `Retriever` trait in `retriever.rs` to implement a plugin architecture for external system integration. *(Done: `retriever.rs:49`, ADR-041 — Local-FS / Tailnet-FS / Web planes implement it.)*

## General Architecture & Observability
- [x] **Observability:** Integrate `tracing` and `tracing-subscriber` crates to provide robust, leveled debug logging across all crates (distinct from the LLM trace JSONL). *(Sprint 72, ADR-063: `ferric-cli` owns a stderr, quiet-by-default subscriber (`-v`/`FERRIC_LOG`); `ferric-loop`/`ferric-tools`/`ferric-provider` emit spans + leveled events. Guard stays pure — its denials are logged at the registry chokepoint.)*
- [ ] **Tool Registration Macros:** Refactor `ferric-tools` to use a procedural macro (e.g., `#[ferric_tool]`) or `typetag` to automatically discover and register tools, reducing boilerplate in `builtin/mod.rs`.
- [ ] **Parallel Tool Execution:** Extend the `Tool` trait in `ferric-core` to declare read/write side-effects, allowing `ferric-loop` to safely dispatch parallelizable tool calls (like multiple `read_file`s) concurrently.
- [ ] **Provider Expansion:** Add native support for Anthropic (`Claude 3.5`) and Gemini (`Gemini 1.5 Pro`) backends in `ferric-provider`, expanding beyond the current OpenAI-compatible implementation.
