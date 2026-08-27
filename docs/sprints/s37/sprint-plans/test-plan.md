Finalized - DO NOT EDIT

# Sprint 37 Test Plan

## Unit Tests

### T-3701 unit tests
- `default_complete_streaming_matches_complete_with_text`: a scripted `MockProvider`-shaped test
  provider without an override, given a text-bearing completion, → `complete_streaming` returns
  the identical `Completion` `complete()` would, and the sink recorded exactly one `Text` delta.
- `default_complete_streaming_no_delta_for_tool_only`: a tool-calls-only completion (no text) →
  zero deltas fired.
- `default_complete_streaming_never_fires_tool_named` (test-critique C-007): a tool-calls-only
  completion → the sink recorded zero `ToolNamed` deltas too, not just zero `Text` — makes the
  "activity signal is real-streaming-only" scoping decision an explicit, checked invariant.
- Stubs: a minimal test `Provider` impl (or reuse `MockProvider` directly, since it doesn't
  override `complete_streaming`).

### T-3702 unit tests (the sprint's core test surface)
- `tool_named_fires_once`: feed `scan()` growing accumulated text across multiple calls once
  `"tool":"task_complete"` is present; assert `ToolNamed` appears in the first call where it's
  decodable and never again in subsequent calls.
- `summary_streams_incrementally`: feed `scan()` a sequence of growing prefixes of
  `{"tool":"task_complete","args":{"summary":"Hello, world!"}}`; assert the cumulative concatenation
  of all emitted `Text` deltas equals `"Hello, world!"` and no delta repeats already-emitted chars.
- `non_task_complete_tool_never_emits_text`: accumulated text for `{"tool":"write_file","args":
  {"path":"a.txt","content":"secret data"}}` → only `ToolNamed("write_file")` fires, never a `Text`
  delta (proves `content`/other arg fields are never leaked as if they were prose).
- `tool_field_always_precedes_args_content` (test-critique C-002): a regression pinning the safety
  argument itself — feeds a completion shaped so a decoy string (`"path":"my_task_complete_file.txt"`)
  would appear BEFORE `"tool":"..."` if field order were ever violated; asserts `ToolNamed` is only
  ever emitted for the real, schema-first `tool` key, never a false-positive from `args` content.
  (Belt-and-suspenders alongside the `action_schema` field-order tests already in `grammar.rs`.)
- `escaped_characters_decode_correctly`: a summary value containing `\"`, `\\`, `\n`, `\t` decodes
  to the correct literal characters in the emitted deltas.
- `ambiguous_trailing_escape_is_held_back`: accumulated text ending in an odd number of trailing
  `\` (ambiguous: could be `\\` fully escaped or the start of `\"`/`\n` pending the next char) →
  that trailing byte is NOT emitted in the current call; a follow-up call with the resolving
  character emits it correctly with no corruption.
- `ambiguous_trailing_unicode_escape_is_held_back` (test-critique C-001): a `\uXXXX` escape split
  across `scan()` calls at EACH of its possible cut points (after `\`, `\u`, `\u0`, `\u00`,
  `\u000`) → nothing is emitted for the incomplete sequence at any cut point; the correct decoded
  character emits only once all 4 hex digits are present, with no partial/garbled output at any
  intermediate call.
- `closing_quote_stops_emission`: accumulated text through the summary's closing unescaped `"` plus
  trailing `}}`  → no further `Text` deltas fire for the trailing JSON syntax.
- Stubs: none (pure string-in, `Vec<StreamDelta>`-out function).

### T-3703 unit tests
- `accumulate_lines_content_only`: SSE lines carrying only `delta.content` chunks (NativeTools/
  TextXml shape) → final `Completion.message.text` equals the concatenation, `on_delta` fired per
  chunk.
- `accumulate_lines_tool_call_only`: SSE lines carrying `delta.tool_calls` fragments → final
  `Completion.message.tool_calls` matches what `complete()`'s non-streaming parser would produce
  for the equivalent full JSON.
- `accumulate_lines_done_terminates`: a `data: [DONE]` line ends accumulation without erroring.
- `accumulate_lines_ignores_blank_and_comment_lines`: blank lines and `: comment` SSE lines are
  skipped, not treated as malformed chunks.
- `accumulate_lines_constrained_json_uses_scanner`: when a `ConstrainedJsonScanner` is supplied,
  `on_delta` receives the scanner's `ToolNamed`/`Text` deltas (not raw JSON fragments) as content
  accumulates.
- Stubs: none for the pure accumulator — plain `&[&str]` inputs.

## Integration Tests
### T-3703 E2E — real `.bytes_stream()` I/O
- `complete_streaming_over_real_tcp_stream`: a hand-rolled `tokio::net::TcpListener` bound to
  `127.0.0.1:0` (ephemeral port), accepting one connection and writing a canned raw HTTP response
  with an SSE body (`Content-Type: text/event-stream`, `Connection: close` — **not**
  `Content-Length`, since an SSE body is unbounded-length and a wrong/missing length would make
  `reqwest` hang waiting for more bytes instead of completing (test-critique C-009) — several
  `data: {...}` chunks + `[DONE]`). `OpenAiProvider::complete_streaming` pointed at that address
  returns the expected `Completion` and the sink recorded the expected delta sequence in order.
  No new mocking dependency — a real local socket, in the spirit of sprint 36's `mcp_stdio_e2e`
  real-process testing preference (a genuinely new technique for this crate, not a repeat of that
  exact pattern). Feature-gated (`backend-openai`), `#[tokio::test]`.

### T-3704 integration
- `stream_sink_none_regression`: the full existing `ferric-loop` test suite (`constrained_loop.rs`,
  `grammar_loop.rs`, `loop_core.rs`, etc.) passes unchanged with no code modification needed — the
  new `stream_sink` field defaults every existing `RunArgs{...}` call site to explicit `None`,
  proving byte-identical behavior.
- `stream_sink_some_drives_dispatch`: a test provider overriding `complete_streaming` to fire a
  known `ToolNamed`+`Text` sequence before returning a `task_complete` `Completion`; assert the
  sink recorded exactly that sequence AND the loop's outcome (`StopReason::TaskComplete`,
  `final_text`) is correct — proves the loop's dispatch logic is unaffected by streaming.
- `streaming_retry_does_not_replay_failed_attempt_deltas` (test-critique C-006 — the
  research-identified retry risk, now concretely tested): a test provider whose `complete_streaming`
  fires one `Text` delta then returns `ProviderError::RetryableBackend` on attempt 1, and succeeds
  (different deltas) on attempt 2; asserts the sink recorded attempt-1's delta exactly once (not
  duplicated by the retry) and the final `Completion` matches attempt 2's, not a merge of both.

## End-to-End Tests
- **Status:** possible (via `--mock` and the T-3703 fake-server harness — no real GGUF model
  required for automated E2E).
- `cli::stream_flag_mock_no_duplication` (`crates/ferric-cli/tests/cli.rs`): `ferric query --mock
  --stream "..."` → stdout contains the mock's final text exactly once (not duplicated by both the
  streamed sink and the final `println!`).
- `cli::stream_flag_omitted_is_unchanged`: existing `mock_query_end_to_end` and friends are run
  unmodified — proves `--stream`'s absence doesn't alter default behavior (the T-3705 regression
  clause).
- Real-backend streaming smoke (a live llama-server, watching text arrive incrementally rather than
  all at once) is a **manual verification step**, not automated — matches the project's established
  no-live-backend-CI position (ADR-045). The T-3703 fake-server E2E covers the real-wire-protocol
  proof deterministically instead.

## Build / Lint (all tasks)
- `cargo test --workspace` green (default features).
- `cargo test -p ferric-provider --features backend-openai` green (T-3703's feature-gated tests,
  incl. the fake-server E2E).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo clippy -p ferric-provider --features backend-openai --all-targets -- -D warnings` clean.
- `cargo fmt --all --check` clean.
