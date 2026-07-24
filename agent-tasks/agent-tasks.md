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
- [x] **Interactive "Accept Edits" Mode:** Pause the driver to preview a mutating tool call, awaiting user confirmation before flushing to disk. *(Sprint 79, ADR-070 — `ferric query --accept-edits`: a `RunArgs.edit_approver` callback previews each Write/Execute call at the dispatch gate; reject skips it and reports a rejection to the model. A stdin y/N approver in the CLI. Preview-based v1; full unified diffs deferred.)*
- [x] **Direct Terminal Passthrough:** Allow commands prefixed with `!` or `/run` in the chat UI to execute instantly via `shell_exec` without LLM roundtripping. *(Sprint 78, ADR-069 — `ferric chat` `!<cmd>`/`/run <cmd>` runs through the guarded `shell_exec` chokepoint (command denylist still enforced), human-initiated, no LLM, not folded into talk history. A lazily-created tokio runtime backs the sync REPL.)*
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

## Verification remediation — from `docs/verification-2026-07.md` (ADR-072)
Ordered by consequence. Each was verified against a green toolchain; A1/A3/A6 and
the Dark Matter divergence were each demonstrated by a test written to fail.

- [x] **A3 — stop destroying the user's git index.** `ferric-vcs/src/lib.rs:36-52` runs `git add -A` then `git reset`, once per turn via `run.rs:256`. Measured: a staged file is unstaged on turn 1. Fix is a temporary `GIT_INDEX_FILE`, **not** the `git read-tree HEAD` the source comment suggests — that was measured to destroy the index identically. Also gate `git clean -fd` in `revert` behind confirmation, and delete the shipped think-aloud comment. *(Done sprint 83, ADR-073 — private GIT_INDEX_FILE (NOT `read-tree HEAD`, measured to be equally destructive); plus a containment guard for the ancestor-repo escape the audit missed, and a confirmation prompt on revert.)*
- [x] **A1 — restore tool-output truncation.** `ToolOutput.for_model` is computed then discarded (`run.rs:756` `_for_model`); the `ToolResult` event carries `full`, so the projector feeds untruncated output back every turn. Measured 20,028 chars where ADR-002 promises 4,000. Add the cross-crate test that would have caught it, and rename `truncation_tests.rs` or the new one so the two truncations stop colliding. *(Done sprint 83, ADR-073 — applied in the projector where the context window is assembled; 4 tests covering both halves of the contract.)*
- [x] **A2 — taint the content, not the provenance.** `query.rs:927-928` calls `taint_str(&d.source)` (harness-stamped path) while injecting `d.summary` (untrusted). Taint `summary` + each `claims[].quote`. Also decide whether `d.claims` should reach the prompt at all — they are currently built and dropped. *(Done sprint 83, ADR-073 — `taint_text` marks summary + claims + quotes at line/sentence granularity, because whole-summary tainting would never have matched a lifted fragment.)*
- [x] **A4 — de-panic `manage_task`.** 9 lock `.unwrap()`s in `builtin/manage_task.rs` + 3 in `builtin/task_registry.rs`; one poisoned mutex aborts every later call. Also `Handle::current()`/`block_in_place` panic off a multi-thread runtime while `ferric-loop` is executor-agnostic, and `send_input` races on stdin take/restore. Cover the panic paths, not just the happy path `tests/background_tasks.rs` already has. *(Done sprint 84, ADR-074 — all 12 lock unwraps gone (poison recovered, not fatal); the two runtime panics are now tool errors via `blocking::block_on_ambient`; the send_input race removed by borrowing stdin in place. Found and fixed the same panic pair in `shell_exec` (Ring 0), plus a colliding-task-id defect that was in no report.)*
- [x] **A7 — wire `RequireApproval` to `EditApprover`.** `registry.rs:207` degrades it to `Deny` commenting "not wired", while ADR-070 shipped exactly that mechanism at the dispatch site. *(Done sprint 84, ADR-074 — via an `ApprovalRequest`/`SinkApprover` callback owned by ferric-tools, so the chokepoint can ask a human without depending on the loop. No approver still denies, but says why.)*
- [x] **A5 — invert the sandbox default.** `WebRetriever::new()` ships `enforce_runsc:false` + `proxy_url:None` with `--network bridge`. Make the airlock opt-out, not opt-in. *(Done sprint 84, ADR-074 — default is now no network + gVisor required; `NetworkPolicy` makes unrestricted egress a variant you must name. argv construction split into a pure `docker_args()` so it is testable without Docker.)*
- [x] **A6 — fix short-token reference queries.** `fetch_reference::tokenize` drops tokens of <=2 chars, so "Go"/"AI"/"k8" match nothing. Also `t.len() > 2` is byte length, so short multibyte terms are mishandled. *(Done sprint 83, ADR-073 — terms under 3 chars match whole words; longer terms keep substring/stem matching.)*
- [x] **C1 — `run_with_provider` takes `RunArgs`.** 18 positional params whose body immediately re-packs into a struct that already exists; removes 5 `too_many_arguments` allows. Highest value-to-risk item in the report. *(Done sprint 84, ADR-074 — plus C2 (post_turn extracted), C3 (ferric-vcs honestly sync, tokio dropped), C4/C5 (registry removal path, status label). Suppressions 5 -> 1.)*
- [x] **Cleanups, all proven safe.** Remove the 6 unused deps (verified: workspace compiles without them); delete the root `test-sweep-prompt.txt` duplicate; rename `prompts/protocol-unified-grammar.md` to `protocol-text-xml.md`; drop `LoopState.registry_tools`, `SandboxConfig::default()`, and either surface or delete `_parse_error`. *(Done sprint 83, ADR-073 — 6 deps removed (verified by compiling without them), duplicate file deleted, prompt atom renamed, `registry_tools` dropped; `SandboxConfig::default()` and `_parse_error` resolved by USE rather than deletion.)*
- [x] **Dark Matter contract decision.** Ferric requires `query`; DM requires `target` and makes `query` optional, so a DM-legal call is hard-rejected. Ferric returns markdown; DM specifies `{chunks:[{uri,text,score}], truncated}`. Either DM narrows INV-3 to one-corpus-per-stage or Ferric grows `target`. Harden DM's `test_ferric_citations_resolve`: it checks two files exist, neither being `fetch_reference.rs`, and passes on skip. *(Partly done sprint 84, ADR-074 — Ferric accepts `target`, `query` is optional, truncation is signalled, and DM's verifier now reads the real descriptor. STILL OPEN: the return shape (DM's JSON envelope vs markdown), which needs an A/B because it changes what every small model sees.)*
