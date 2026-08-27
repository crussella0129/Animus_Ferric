Finalized - DO NOT EDIT

# Sprint 37 Build Plan

## Schema Tree
- Sprint Goal: streaming inference as a base architectural choice
  - Provider trait extension
    - T-3701: `StreamDelta` + `Provider::complete_streaming` default method
  - Constrained-JSON incremental extraction (the sprint's core novel problem)
    - T-3702: `ConstrainedJsonScanner` (pure)
  - HTTP-valve implementation
    - T-3703: `OpenAiProvider::complete_streaming` (SSE accumulation + real I/O)
  - Loop wiring
    - T-3704: thread the display sink through the agent loop
  - CLI wiring
    - T-3705: `ferric query --stream`
  - Docs
    - T-3706: ADR-047 + docs

## Execution Sequence

### T-3701: `StreamDelta` + `Provider::complete_streaming` default method
- **Touches:** `crates/ferric-provider/src/types.rs`, `crates/ferric-provider/src/traits.rs`
- **Depends on:** (none)
- `StreamDelta { Text(String), ToolNamed(String) }` in `types.rs`. `Provider` gains a
  `complete_streaming` method with a default body: call `self.complete(request).await?`, then if
  the completion has text fire `on_delta(&StreamDelta::Text(text))` once, and return the
  completion unchanged. Every existing provider (`MockProvider`, `MistralRsProvider`) inherits this
  for free — zero code change, zero behavior change.
- **Success criterion (EARS):**
  - **WHEN** a provider has no override, **THEN** `complete_streaming` **SHALL** return the same
    `Completion` `complete()` would for the same request.
  - **WHEN** the completion has text, **THEN** the default impl **SHALL** fire exactly one `Text`
    delta carrying it; **WHEN** it has none, **THEN** no delta fires.
  - **WHEN** a provider has no override and the completion carries `tool_calls` with no text
    (test-critique C-007, making an implicit gap explicit), **THEN** the default impl **SHALL NOT**
    fire `ToolNamed` — the early tool-name activity signal is a real-streaming-only feature,
    surfaced only by `OpenAiProvider`'s actual implementation (T-3703), not the default.
- **Notes:** `on_delta: &dyn Fn(&StreamDelta)` (not `FnMut`) — implementors needing to track state
  across calls capture a `Cell`/`RefCell`, keeping `Provider` simple and dyn-compatible (ADR-003).

### T-3702: `ConstrainedJsonScanner` — the incremental summary-field extractor (pure)
- **Touches:** `crates/ferric-provider/src/stream_scan.rs` (new), `crates/ferric-provider/src/lib.rs`
  (module wiring + re-export)
- **Depends on:** T-3701 (uses `StreamDelta`)
- Fed the FULL accumulated raw JSON text on each `scan()` call (not a diff); internally tracks what
  it has already emitted. Recognizes `"tool":"<name>"` → emits `ToolNamed` once. **Safe against a
  false-positive match inside an unrelated string value (test-critique C-002):** the completion
  text `scan()` receives is always the bare top-level action object itself — never nested inside
  another field — and `action_schema` (`grammar.rs`) always emits `tool` as the object's FIRST
  property (ADR-016's `preserve_order` field-ordering discipline). So `"tool":"..."` is always the
  first key-value pair seen; no `args` string value (e.g. a decoy like `"path":
  "my_task_complete_file.txt"`) can precede it and be misread as the tool name.
  When `tool == "task_complete"`, recognizes entering `"summary":"` and live-decodes JSON
  string-escape sequences (`\"`, `\\`, `\n`, `\t`, `\uXXXX`). **Escape-boundary withhold rule
  (test-critique C-001, precisely stated):** hold back from the START of any incomplete escape
  sequence at the end of the accumulated text — 1 character for a lone trailing `\`, and up to 5
  for a `\uXXXX` split at any point (`\`, `\u`, `\u0`, `\u00`, `\u000` are all incomplete and must
  withhold everything from the `\` onward); only a `\uXXXX` with all 4 hex digits present decodes.
  **Known accepted edge case (test-critique C-003):** if a turn is later discarded after some
  `summary` text has already streamed (e.g. `finish_reason=="length"` truncation mid-summary,
  `run.rs`'s truncated-action nudge-and-retry path), the already-printed partial text has no
  retraction mechanism this increment — accepted as a narrow, low-probability rough edge (ADR-018's
  per-tier token budgets leave headroom over a one-sentence summary, making mid-summary truncation
  unlikely in practice); revisit only if it proves disruptive in use.
- **Success criterion (EARS):**
  - **WHEN** accumulated text contains a complete `"tool":"<name>"` field, **THEN** `scan()`
    **SHALL** emit exactly one `ToolNamed(name)` (first call only, never repeated).
  - **WHEN** the tool is `task_complete` and new decodable `summary` characters are available,
    **THEN** `scan()` **SHALL** emit a `Text` delta containing only the newly-available decoded
    characters.
  - **WHEN** the tool is NOT `task_complete`, **THEN** `scan()` **SHALL** never emit `Text` for
    other args fields.
  - **WHEN** the trailing text ends mid-escape-sequence (a lone `\` OR a partial `\uXXXX` at any of
    its 6 characters), **THEN** `scan()` **SHALL** hold back everything from the start of that
    incomplete escape, emitting nothing wrong/garbled, until a later call resolves it.
  - **WHEN** the summary's closing unescaped quote is reached, **THEN** `scan()` **SHALL** stop
    emitting further text for that call even if more accumulated text follows.

### T-3703: `OpenAiProvider::complete_streaming` (SSE accumulation + real I/O)
- **Touches:** `crates/ferric-provider/src/openai.rs`; `Cargo.toml` — `reqwest` gains the `stream`
  feature (needed by `complete_streaming`'s production code path, `.bytes_stream()`); `tokio`'s
  `net` feature is added **dev-dependency-scoped, test-only** (test-critique C-004: verified
  `.bytes_stream()` returns a plain `futures_core::Stream` polled by the caller's existing runtime
  — it does NOT itself need `tokio::net`. Only T-3703's OWN fake-server E2E test needs
  `tokio::net::TcpListener` directly. `ferric-provider`'s `Cargo.toml` already scopes `tokio` to
  `features = ["time"]`; add `net` under `[dev-dependencies]` or an equivalent test-only feature
  set, NOT the production `backend-openai` feature's tokio requirement — keeps ADR-004's allowlist
  discipline honest).
- **Depends on:** T-3701, T-3702
- Sets `"stream": true` in the request body (reuses `build_body`). One coherent unit of review
  spanning a pure core + a thin I/O shell over one file (test-critique C-005: not claiming sprint
  36 precedent for this shape — justified standalone: splitting the pure accumulator, the async
  wrapper, and the feature-flag change into separate tasks would create artificial dependency
  chains for what is fundamentally one file's one new capability): a pure inner function
  (`accumulate_lines` or equivalent) takes a sequence of already-decoded SSE `data:` line strings +
  a `ConstrainedJsonScanner` (when the request carries a `Constraint::JsonSchema`, else `None`) +
  `on_delta`, producing the final `Completion` — unit-testable with plain `&[&str]`, no networking.
  `complete_streaming` is a thin async wrapper: reads `.bytes_stream()`, buffers into complete
  lines (SSE lines don't align with TCP/HTTP chunk boundaries), feeds them to the pure accumulator.
  **E2E framing note (test-critique C-009):** the fake server's canned response must use
  `Connection: close` (or `Transfer-Encoding: chunked`) rather than a `Content-Length` header —
  an SSE body is unbounded-length, and a wrong/missing length will make `reqwest` hang waiting for
  more bytes instead of completing.
- **Success criterion (EARS):**
  - **WHEN** `complete_streaming` is called, **THEN** the request body **SHALL** set `stream:true`.
  - **WHEN** the stream completes normally, **THEN** the returned `Completion` **SHALL** be
    identical in content to what `complete()` produces for an equivalent non-streaming response
    (message text, tool_calls, truncated flag).
  - **WHEN** `delta.tool_calls`/`delta.content` chunks arrive, **THEN** they **SHALL** accumulate
    into the same final shapes `complete()`'s parser produces today.

### T-3704: Thread the display sink through the agent loop
- **Touches:** `crates/ferric-loop/src/run.rs` (new `RunArgs.stream_sink` field),
  `crates/ferric-loop/src/backoff.rs` (new `complete_streaming_with_backoff`)
- **Depends on:** T-3701
- `RunArgs` gains `pub stream_sink: Option<&'a dyn Fn(&StreamDelta)>`. `complete_streaming_with_backoff`
  mirrors `complete_with_backoff`'s retry loop, calling `provider.complete_streaming(request,
  on_delta)` instead of `provider.complete(request)`. The turn loop calls the streaming variant
  when `stream_sink` is `Some`, else the existing `complete_with_backoff` when `None`.
- **Success criterion (EARS):**
  - **WHEN** `stream_sink` is `None`, **THEN** turn execution **SHALL** be byte-identical to today.
  - **WHEN** `stream_sink` is `Some`, **THEN** the turn **SHALL** call `complete_streaming` and the
    loop's dispatch/validation logic **SHALL** operate on the resulting `Completion` exactly as it
    does for a non-streaming turn.
  - **WHEN** `complete_streaming` returns a retryable error mid-stream (test-critique C-006 — the
    research-identified retry risk, now a concrete clause), **THEN**
    `complete_streaming_with_backoff` **SHALL** retry with a fresh request and **SHALL NOT** replay
    already-fired deltas from the failed attempt (each attempt's deltas are independent; a retry
    simply restarts the whole request, exactly as non-streaming retry already does).

### T-3705: `ferric query --stream` CLI wiring
- **Touches:** `crates/ferric-cli/src/query.rs`, `crates/ferric-cli/src/mcp.rs`
- **Depends on:** T-3704
- New `--stream` flag on `QueryArgs`. When set, builds a sink (captured `Cell<bool>` tracking
  whether anything printed for the current call) that prints `Text` deltas to stdout, flushed per
  delta, and `ToolNamed` as a `▸ calling <name>...` line to stderr. `run_query`'s final
  `println!(outcome.final_text)` is skipped when the sink already displayed it. `mcp.rs`'s
  `run_one` passes `None` for the new `run_with_provider`/`RunArgs` parameter — MCP does not stream
  this sprint (ADR-046's stdout-purity constraint).
- **Success criterion (EARS):**
  - **WHEN** `--stream` is passed and the backend streams real content, **THEN** text **SHALL**
    print to stdout as it arrives, not just at the end.
  - **WHEN** `--stream` is passed with `--mock`, **THEN** output **SHALL** still be correct (no
    duplication, no missing text) even though nothing streams incrementally.
  - **WHEN** `--stream` is omitted, **THEN** behavior **SHALL** be byte-identical to today.

### T-3706: ADR-047 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-3701, T-3702, T-3703, T-3704, T-3705
- ADR-047: the `complete_streaming`-with-default-impl design and why, the `ConstrainedJson`
  summary-extraction approach and why it preserves "the harness owns decoding," and the explicit
  follow-on list (MCP streaming, mistral.rs streaming, mid-stream retry, a structured/programmatic
  streaming mode).
- **Success criterion (EARS):**
  - **WHEN** ADR-047 is read, **THEN** it **SHALL** state the callback-vs-Stream decision and
    rationale, and explicitly list what's deferred.
