Finalized - DO NOT EDIT

# Sprint 42 Test Plan

## Unit Tests
### T-4202 unit tests (`crates/ferric-cli/src/chat.rs`, `#[cfg(test)]`)
- `parse_chat_input_do_is_escalate`: `/do fix the bug` → `Escalate("fix the bug")`.
- `parse_chat_input_help_exit_quit`: `/help` → `Help`; `/exit` → `Exit`; `/quit` → `Exit`.
- `parse_chat_input_empty_is_empty`: `""` and `"   "` → `Empty`.
- `parse_chat_input_default_is_talk`: `explain this function` → `Talk("explain this function")`;
  a leading-slash non-command (e.g. `/unknown`) → `Talk("/unknown")` (unknown commands are talked,
  not silently executed).
- `talk_request_has_no_action_channel` (the key structural-safety test): `talk_request(&history,
  &sampling)` returns a `CompletionRequest` with `tools.is_empty()` AND `constraint.is_none()` —
  proving talk mode cannot dispatch a tool.
- `escalation_seeds_replayed_state_from_history`: building the escalation seed from a non-trivial
  history produces a `ReplayedState` whose `messages` equals the current history and whose
  `protocol` matches the config — proving the escalated run continues the SAME conversation.
- Stubs: `MockProvider` (existing `ferric-provider` helper) where a completion is needed.

## Integration Tests
### T-4203 integration (`crates/ferric-cli/tests/cli.rs`, `ferric chat --mock` subprocess)
Uses a NEW stdin-piping harness (plan-critic C-005): `Stdio::piped()` + `child.stdin.write_all(...)`
+ `wait_with_output()` — the suite's first stdin-driven test. `/exit` is handled purely at the parse
layer (no `complete()` call) and the mock is fresh-per-turn (C-001), so no off-by-one can hang the
child.
- `chat_talk_then_exit`: pipe `hello\n/exit\n` to `ferric chat --mock`; asserts a response is
  printed to stdout and the chat-session log file (one, under `.ferric/trace/`) contains a chat
  `session_start` + a talk `note` + `session_end`.
- `chat_do_escalates_to_agentic_loop`: pipe `/do write a file\n/exit\n`; asserts a SEPARATE
  per-escalation trace file (mcp-precedent) contains agentic-loop events (`tool_call` and
  `session_end`) — proving `/do` drove the real constrained loop, not the talk path.
- `chat_help_lists_commands`: pipe `/help\n/exit\n`; asserts stdout names `/do`, `/help`, `/exit`.
- `chat_talk_turn_is_not_dispatched` (structural safety, black-box): a plain talk line whose text
  looks like a tool call (e.g. `write_file to /etc/passwd`) produces ONLY a printed response, and
  NO agentic trace file / NO `tool_call`/`tool_result`/`permission_check` event anywhere — the talk
  path never dispatches. (The load-bearing structural proof is the UNIT test
  `talk_request_has_no_action_channel`; this black-box test confirms the harness doesn't fabricate a
  dispatch — both are jointly necessary, plan-critic C-006.)

### Regression
- Every existing `ferric-cli` test unaffected — `chat` is a new subcommand; `query`/`mcp`/`trace`/
  `server`/`bench` unchanged. The full `cli.rs` + `main.rs` unit suites keep passing.

## End-to-End Tests
- **Status:** possible via `--mock` — covered by the `ferric chat --mock` subprocess tests above
  (the strongest end-to-end proof: a real `ferric` binary, real stdin piping, real trace files),
  filed under Integration rather than duplicated here (sprints 38–41 precedent).
- A real conversational session against a live GGUF model (talk + `/do` with an actual model
  responding) is a **manual verification step**, not automated — matches the project's no-live-
  backend-CI position (ADR-045). `printf 'hello\n/exit\n' | ferric chat --backend openai ...` is the
  manual smoke.

## Build/Lint (all tasks)
`cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt
--all --check`; `--features backend-openai`/`--features backend-mistralrs` builds unaffected.
