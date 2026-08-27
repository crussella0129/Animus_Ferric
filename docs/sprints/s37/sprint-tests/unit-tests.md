# Sprint 37 Unit Tests

All derived from the locked `build-plan.md` EARS clauses (one test per WHEN/THEN/SHALL triple),
incl. the plan-critique's C-001/C-002/C-006/C-007 fixes folded in during the build. All green.

## T-3701 — `StreamDelta` + `Provider::complete_streaming` default method
- `traits::tests::default_complete_streaming_matches_complete_with_text` — the default impl (no
  override) returns the identical `Completion` `complete()` would, firing exactly one `Text` delta.
- `traits::tests::default_complete_streaming_no_delta_for_tool_only` — a tool-calls-only
  completion (no text) fires zero deltas.
- `traits::tests::default_complete_streaming_never_fires_tool_named` — the explicit C-007
  invariant: not just "no Text", specifically no `ToolNamed` either (that signal is
  real-streaming-only, `OpenAiProvider`-only).

## T-3702 — `ConstrainedJsonScanner` (the sprint's core novel logic)
- `stream_scan::tests::tool_named_fires_once` — fires exactly once across many `scan()` calls.
- `stream_scan::tests::summary_streams_incrementally` — cumulative emitted text across growing
  prefixes equals the full summary, no repeats.
- `stream_scan::tests::non_task_complete_tool_never_emits_text` — only `ToolNamed` fires for a
  non-`task_complete` tool; args content (e.g. `write_file`'s `content`) never leaks as prose.
- `stream_scan::tests::tool_field_always_precedes_args_content` — the C-002 regression: `args`
  string content that reads like a tool/task-complete reference doesn't confuse tool-identity
  detection.
- `stream_scan::tests::escaped_characters_decode_correctly` — `\"`, `\\`, `\n`, `\t` decode right.
- `stream_scan::tests::ambiguous_trailing_escape_is_held_back` — a lone trailing `\` is withheld
  until resolved.
- `stream_scan::tests::ambiguous_trailing_unicode_escape_is_held_back` — the C-001 fix: a `\uXXXX`
  split at each of its 6 possible cut points withholds correctly at every point.
- `stream_scan::tests::closing_quote_stops_emission` — the closing unescaped quote stops emission
  even with trailing JSON syntax (`}}`) following.
- `stream_scan::tests::malformed_unicode_escape_stops_gracefully` (test-critique C-001) — a
  genuinely malformed `\u` escape (not merely incomplete) stops decoding cleanly instead of
  stalling forever; fixed a real conflation bug in the process (see `critique.md`).

## T-3703 — `OpenAiProvider::complete_streaming`
- `openai::tests::classify_sse_line_variants` — `data:`/`[DONE]`/blank/comment lines classify
  correctly (pure, no networking).
- `openai::tests::accumulate_lines_content_only` — `delta.content`-only lines accumulate into
  `Completion.message.text`; `on_delta` fires per chunk.
- `openai::tests::accumulate_lines_tool_call_only` — `delta.tool_calls` fragments accumulate into
  the same shape `complete()`'s non-streaming parser produces.
- `openai::tests::accumulate_lines_done_terminates` — the `[DONE]` sentinel classifies distinctly
  from an ignored line.
- `openai::tests::accumulate_lines_ignores_blank_and_comment_lines` — non-data lines are ignored,
  not treated as malformed.
- `openai::tests::accumulate_lines_constrained_json_uses_scanner` — under `ConstrainedJson`,
  `on_delta` receives the scanner's `ToolNamed`/`Text` deltas, never raw JSON fragments.
- `openai::tests::complete_streaming_rejects_invalid_request` (test-critique C-002) — a
  both-tools-and-constraint request is rejected before any network call, zero deltas fired.
- `openai::streaming_e2e::complete_streaming_over_real_tcp_stream` — see E2E section.
- `openai::streaming_e2e::complete_streaming_surfaces_http_error_status` (test-critique C-003) —
  see E2E section.
- `openai::streaming_e2e::complete_streaming_reassembles_a_line_split_mid_write` (test-critique
  C-005) — see E2E section.

## T-3704 — thread the display sink through the loop
- `stream_sink: None` regression: every pre-existing `ferric-loop` test (constrained_loop,
  grammar_loop, loop_core, progress_tests, repetition_tests, terminator_tests, truncation_tests,
  backoff_tests) passes unchanged — the new field is explicit `None` at every existing call site.
- `streaming_tests::stream_sink_some_drives_dispatch` — `stream_sink: Some` calls
  `complete_streaming` and the loop's dispatch/validation still operates correctly on the result
  (`StopReason::TaskComplete`, correct `final_text`), with the sink recording the exact scripted
  delta sequence.
- `streaming_tests::streaming_retry_does_not_replay_failed_attempt_deltas` — the C-006 fix: a
  retryable mid-stream error retries fresh; the sink shows attempt-1's delta exactly once (not
  duplicated) followed by attempt-2's, and the final `Completion` is attempt-2's.

## T-3705 — `ferric query --stream`
- `cli::stream_flag_mock_no_duplication` — `--mock --stream` produces the final text exactly once
  (no duplication between the streamed sink and the final echo).
- `--stream` omitted: every existing CLI test (`mock_query_end_to_end`, file-routing tests, ring
  tests, etc.) passes unchanged — the regression clause.

## Result
`cargo test -p ferric-provider` (default): 18 passed (17 + the C-001 fix's test — pure logic, no
feature needed). `cargo test -p ferric-provider --features backend-openai`: 38 passed (up from the
original 34 after the test-critic's C-001/C-002/C-003/C-005 fixes — see `critique.md`).
`cargo test -p ferric-loop`: 22 passed (incl. the 2 streaming integration tests). `cargo test -p
ferric-cli`: 42 passed; `--features backend-openai`: unaffected. `cargo test --workspace`: all
green.
