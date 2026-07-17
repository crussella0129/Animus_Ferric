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
- [ ] Implement Dynamic Denylist Configuration in `src/denylist.rs` to read from a `.ferricignore` file.

## ferric-loop
- [x] Extract Loop State from `run.rs` into a dedicated `LoopState` struct.
- [x] Decouple Driver from Logic by breaking down the monolithic `while` loop into a `step(&mut LoopState) -> Result<TurnOutcome>` function.

## ferric-prompt
- [ ] Implement a templating engine (`minijinja` or `askama`) to replace manual `format!()` strings in `src/lib.rs`.

## Advanced Harness Features (Plan, Stop, Revert, Dream)
- [x] **Plan Mode**: Implement an `ActionProtocol::Plan` that strips write-permissions and provides a `submit_plan` terminator, effectively utilizing the robust constraint and tool-call mechanics to safely enforce an initial planning phase.
- [x] **Graceful Interrupts (Stop):** Integrate `tokio::signal::ctrl_c` handling in the CLI driver and pass a cancellation token to `LoopState` to abort execution and gracefully commit a `SessionEnd` trace.
- [x] **Time Travel (Revert):** Implement a lightweight VCS wrapper (`ferric-vcs`) to automatically orphan/stash workspace states tied to trace `TurnEnd` events, allowing the CLI `revert <turn_id>` command to rollback both the workspace and the replay trace.
- [x] **Dream Mode:** Create an asynchronous offline `ferric dream` worker that parses historical `.ferric/traces`, extracts high-value signals/patterns, and consolidates them into a persistent memory context (`MEMORY.md` or `.ferric/knowledge/`).
- [x] **Configurable Hooks:** Introduce a `ferric.toml` or `hooks/` system for synchronous pre-turn, post-turn, or on-error scripts.
- [x] **Background Task Management:** Add detached `tokio` process management to `shell_exec` and expose a `manage_task` built-in tool.
- [ ] **Agent Delegation Structure (ICM):** Build a `ferric-icm` orchestrator crate that orchestrates sub-agents entirely via sequential local folders (`01_research/`, `02_script/`) and their `CONTEXT.md` files.
- [ ] **Interactive "Accept Edits" Mode:** Pause the driver to display unified diffs via a `PendingEdit` state, awaiting user confirmation before flushing to disk.
- [ ] **Direct Terminal Passthrough:** Allow commands prefixed with `!` or `/run` in the chat UI to execute instantly via `shell_exec` without LLM roundtripping.
- [ ] **Agentic Cron Jobs:** Introduce a `.ferric/cron/` directory and background watcher to schedule periodic agent tasks (e.g., `/dream every 12 hours`).

## ferric-provider
- [ ] Optimize SSE streaming in `src/stream_scan.rs` using `bytes::BytesMut` and `serde_json::StreamDeserializer` instead of `String` buffering.

## ferric-research
- [ ] Define a `Retriever` trait in `retriever.rs` to implement a plugin architecture for external system integration.
