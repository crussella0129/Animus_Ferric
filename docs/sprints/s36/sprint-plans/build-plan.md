Finalized - DO NOT EDIT

# Sprint 36 Build Plan

## Schema Tree
- Sprint Goal: `ferric mcp` — the ADR-005 security call, then the MCP-stdio server
  - Shared query-execution refactor
    - T-3601: Separate provider construction from loop execution
    - T-3602: Extract the launch-time-fixed run-config builder
  - MCP protocol core
    - T-3603: JSON-RPC 2.0 message types + stdio framing
    - T-3604: `initialize` + `tools/list` handlers
    - T-3605: `tools/call` handler for `ferric_query`
  - CLI wiring
    - T-3606: `McpArgs` + `Command::Mcp` + `run_mcp` entrypoint
  - Docs
    - T-3607: ADR-046 + docs

## Execution Sequence

### T-3601: Separate provider construction from loop execution in `query.rs`
- **Touches:** `crates/ferric-cli/src/query.rs`
- **Depends on:** (none)
- Today `drive_real` calls `create_provider(&args.backend_opts).await` inside the same
  `runtime.block_on` block that also runs the loop — construction and execution are fused
  per-call. Split so a provider can be built once and the loop run many times against it.
- **Success criterion (EARS):**
  - **WHEN** `ferric query` runs against a real backend, **THEN** its behavior and output SHALL
    be unchanged (regression against existing tests + `--mock` smoke).
  - **WHEN** a caller supplies an already-constructed `&dyn Provider`, **THEN** the extracted
    function SHALL run one loop execution without reconstructing a provider.
- **Notes:** `Provider::complete(&self, ...)` takes `&self` (confirmed in
  `ferric-provider/src/traits.rs`), so a `Box<dyn Provider + Send + Sync>` is safely reusable
  across sequential calls — no interior-mutability concerns. The `tokio::runtime::Runtime` built
  around the provider-construction `block_on` is ALSO reused, not just the provider: T-3606's
  `run_mcp` builds one `Runtime` at launch and calls `runtime.block_on(...)` once per subsequent
  `tools/call` (a `Runtime` supports repeated sequential `block_on` calls on the same instance —
  this is standard tokio usage, not a new pattern).

### T-3602: Extract the launch-time-fixed run-config builder
- **Touches:** `crates/ferric-cli/src/query.rs`
- **Depends on:** (none)
- Extract the capability-inference / protocol-selection / profile-read-back / `RunPolicy` /
  sampling / system-prompt-composition block (today inline in `run_query`, roughly the caps
  computation through `composed`/`system_prompt`/`lineage`) into a function parameterized by the
  shared subset of `QueryArgs` (everything except `prompt`/`files`).
- **Success criterion (EARS):**
  - **WHEN** `ferric query` builds its run configuration, **THEN** the extracted function SHALL
    produce identical `RunPolicy`/`ActionProtocol`/sampling values to today's inline logic.
  - **WHEN** called once with MCP's launch-time args, **THEN** the resulting config SHALL be
    reusable across multiple subsequent loop executions without rebuilding.
- **Notes:** registry (`Registry::new()` + `register_builtin_tools`) has no per-call state, so it
  also belongs in this once-built config. **Accepted tradeoff (deliberate, not accidental):**
  because `ferric mcp` is a long-running process, the profile read-back
  (`ferric_bench::read_profile`, which drives `measured_level`/`calibrated_ring` per ADR-029) is
  computed ONCE at server launch and held for the life of the process — a `ferric bench
  --calibrate-rings` run that updates `model_profiles.json` while an MCP server is already running
  will NOT be picked up until the server restarts. This is a real behavioral divergence from
  `ferric query` (which re-reads the profile file on every invocation) and is intentional, matching
  the same launch-time-fixed philosophy already applied to workspace/backend/model — recorded
  explicitly in ADR-046 (T-3607), not left implicit.

### T-3603: JSON-RPC 2.0 message types + stdio framing
- **Touches:** `crates/ferric-cli/src/mcp.rs` (new)
- **Depends on:** (none)
- Typed `Request { jsonrpc, id, method, params }` / `Response { jsonrpc, id, result | error }` /
  a notification variant (no `id`, no response expected). Newline-delimited read from stdin,
  newline-delimited write to stdout.
- **Success criterion (EARS):**
  - **WHEN** a line of valid JSON-RPC is read, **THEN** it SHALL parse into a typed
    Request/Notification, distinguished by presence of `id`.
  - **WHEN** a line fails to parse, **THEN** a JSON-RPC `-32700 Parse error` response SHALL be
    written to stdout.
  - **WHEN** any response is written, **THEN** it SHALL go to stdout only; all diagnostics SHALL
    go to stderr (never `println!` for logs — would corrupt the protocol stream).

### T-3604: `initialize` + `tools/list` handlers
- **Touches:** `crates/ferric-cli/src/mcp.rs`
- **Depends on:** T-3603
- `initialize` responds with a fixed protocol version constant + `{"tools":{}}` capabilities +
  `serverInfo` (name `ferric`, `CARGO_PKG_VERSION`). `tools/list` responds with exactly one tool,
  `ferric_query`, whose JSON-Schema input is `{prompt: string (required), files?: string[]}`.
- **Success criterion (EARS):**
  - **WHEN** `initialize` is received, **THEN** the server SHALL respond with its fixed protocol
    version and tools capability.
  - **WHEN** `tools/list` is received, **THEN** the response SHALL list exactly one tool whose
    schema has no `workspace`, `backend`, or `model` property.

### T-3605: `tools/call` handler for `ferric_query`
- **Touches:** `crates/ferric-cli/src/mcp.rs`, `crates/ferric-cli/src/query.rs`
- **Depends on:** T-3601, T-3602, T-3604
- Parses `{name, arguments}`. For `ferric_query`: routes `files` through a **shared file-routing
  function extracted from `run_query`'s inline loop** (today `query.rs`'s per-file loop over
  `classify_path`/`decide_attachment` building `media_parts`/`prompt_suffix` is inline, not an
  independently callable function — extract it as part of this task so `mcp.rs` calls the same
  function `query.rs` calls, rather than re-implementing the orchestration around those two pure
  `ferric_core` calls). Opens a fresh per-call trace session (`.ferric/trace/mcp-{ms}.jsonl`), runs
  one loop execution via T-3601's function against T-3602's shared config and the launch-time
  provider. Success → `{content:[{type:"text",text:<final_text>}], isError:false}`. Loop/provider
  error → `isError:true` with the message. Unknown tool name → JSON-RPC `-32602` error.
- **Success criterion (EARS):**
  - **WHEN** `tools/call` names `ferric_query` with a valid `prompt`, **THEN** the server SHALL
    run one constrained-loop execution and return its final text.
  - **WHEN** the loop errors on one call, **THEN** that call's result SHALL carry `isError:true`
    without crashing the process, AND the server SHALL continue accepting and correctly serving
    subsequent `tools/call` requests in the same session.
  - **WHEN** an unknown tool name is given, **THEN** the server SHALL respond with a JSON-RPC
    error, not attempt execution.
  - **WHEN** `files` entries are supplied, **THEN** they SHALL route through the shared
    file-routing function (the same attach/fold/skip decision `ferric query --file` uses), not a
    re-implementation of it.

### T-3606: `McpArgs` + `Command::Mcp` + `run_mcp` entrypoint
- **Touches:** `crates/ferric-cli/src/main.rs`, `crates/ferric-cli/src/mcp.rs`
- **Depends on:** T-3602, T-3605
- `McpArgs` mirrors `QueryArgs` minus `prompt`/`files` — this explicitly INCLUDES `mock: bool`
  (`--mock`), matching `ferric query`'s flag. `run_mcp` builds workspace/registry once, builds ONE
  `tokio::runtime::Runtime` and either a `MockProvider` (via the same protocol-scripted
  `mock_provider()` helper `query.rs` already has, when `--mock` is set) or a real provider via
  `create_provider` (run once through the Runtime), then serves a blocking stdin-read → dispatch
  (via `runtime.block_on` per call, reusing the same Runtime instance — see T-3601 notes) →
  stdout-write loop until EOF.
- **Success criterion (EARS):**
  - **WHEN** `ferric mcp` launches with valid args, **THEN** it SHALL construct the provider and
    the tokio Runtime exactly once and then serve requests until stdin closes.
  - **WHEN** `--mock` is passed, **THEN** `run_mcp` SHALL construct a `MockProvider` (same
    protocol-scripted behavior as `ferric query --mock`) instead of calling `create_provider`.
  - **WHEN** stdin reaches EOF, **THEN** the process SHALL exit cleanly (`ExitCode::SUCCESS`).

### T-3607: ADR-046 + docs
- **Touches:** `decisions.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`,
  `README.md`
- **Depends on:** T-3601, T-3602, T-3603, T-3604, T-3605, T-3606
- ADR-046: the exposed-surface decision, the hand-roll-vs-`rmcp` call and why, and an explicit
  note that chat-mode (the other half of the ADR-011 revision) stays deferred.
- **Success criterion (EARS):**
  - **WHEN** ADR-046 is read, **THEN** it SHALL state the exposed-tool-surface decision and
    explicitly flag that chat-mode is still deferred.
