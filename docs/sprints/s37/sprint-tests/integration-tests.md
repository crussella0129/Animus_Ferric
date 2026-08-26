# Sprint 37 Integration Tests

Component: the streaming path threaded end-to-end through the agent loop
(`crates/ferric-loop/tests/streaming_tests.rs`) — a scripted `Provider` implementing only
`complete_streaming` (never `complete()`), driven through the real `run()` turn loop.

- `stream_sink_some_drives_dispatch` — a `ScriptedStreamingProvider` fires
  `ToolNamed("task_complete")` + `Text("done")` then returns a `task_complete`-shaped
  `Completion`; the full `run()` loop dispatches it correctly
  (`StopReason::TaskComplete`, `final_text == "done"`) and the sink recorded exactly
  the scripted delta sequence, in order. Proves streaming doesn't disturb the loop's
  dispatch/validation logic, which still operates on the same `Completion` shape as
  non-streaming.
- `streaming_retry_does_not_replay_failed_attempt_deltas` — the same provider's first
  `complete_streaming` call fires one delta then returns a retryable error; the second
  (post-backoff-sleep) call fires a different delta and succeeds. Confirms
  `complete_streaming_with_backoff`'s retry composes correctly with the loop: the sink
  sees both attempts' deltas with no duplication, the backoff sleep schedule
  (`RecordingSleeper`) shows exactly one 250ms delay, and the final outcome reflects
  attempt 2 only.

## Result
Both pass (part of `ferric-loop`'s 22). No integration-level failures. The
`stream_sink: None` byte-identical-behavior claim (T-3704's other EARS clause) is
proven by every OTHER test file in the crate — none of which use `Some` — continuing
to pass unchanged after the field was added.
