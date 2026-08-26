# Sprint 36 Integration Tests

Component: the MCP dispatch pipeline (T-3603 framing + T-3604/T-3605 handlers composed through
`McpServer::dispatch`, driven in-process with a scripted provider — no real stdio).

- `mcp::tests::full_handshake_and_call_sequence` — the full MCP lifecycle in order:
  `initialize` → `notifications/initialized` (a notification: dispatch returns `None`, no
  response) → `tools/list` (one tool, `ferric_query`) → `tools/call` (runs one loop, returns the
  mock's `mock run complete`). Proves the handlers compose end to end through one server.
- `mcp::tests::error_then_success_same_session` — two `tools/call`s through the SAME server: a
  `FlakyOnceProvider` errors the first call (`isError:true`) then succeeds the second
  (`isError:false`, `content == "recovered"`, `id == 2`). Proves the serve loop keeps dispatching
  correctly after an error — real recovery, not just "doesn't panic once" (the reworded T-3605
  EARS clause, per the plan-phase critique's C-003).

## Result
Both pass (part of the `ferric-cli` lib's 42). No integration-level failures.
