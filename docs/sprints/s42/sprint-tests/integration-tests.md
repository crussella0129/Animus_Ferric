# Sprint 42 Integration Tests

`crates/ferric-cli/tests/cli.rs` — black-box subprocess tests against the real `ferric` binary,
using a NEW stdin-piping harness (`run_chat_mock`: `Stdio::piped()` + batch `write_all` + close
stdin + `wait_with_output` — the suite's first stdin-driven test). `/exit` is handled purely at the
parse layer (no `complete()` call) and the `--mock` provider is fresh-per-turn, so an off-by-one in
piped lines can't hang the child (plan-critic C-001/C-005). All green (`cli.rs`: 32, up from 28).

## T-4203 — `ferric chat --mock`
- `chat_talk_then_exit` — pipe `hello there\n/exit\n`: the talk response (`[mock chat] ...`) is
  printed; exactly ONE chat-session log file (`chat-*.jsonl`, not `chat-esc-`) with a coherent
  envelope (`session_start` first, `session_end` last, a talk `note` between); NO escalation file
  for a talk-only session.
- `chat_do_escalates_to_agentic_loop` — pipe `/do write a file\n/exit\n`: a SEPARATE
  `chat-esc-*.jsonl` file contains the full constrained agentic loop (`tool_call`, `session_end`),
  and the mock's write actually lands in the workspace (`ferric-mock.txt`) — proving `/do` drove
  `run()` through the guarded path, not the talk path.
- `chat_help_lists_commands` — pipe `/help\n/exit\n`: stdout names `/do`, `/help`, `/exit`.
- **`chat_talk_turn_is_not_dispatched`** (the black-box structural-safety proof) — pipe a talk line
  that LOOKS like a tool call (`write_file to /etc/passwd now\n/exit\n`): it's talked
  (`[mock chat] ...`), opens NO escalation trace file, writes NO `tool_call`/`tool_result`/
  `permission_check` event anywhere, and never touches the workspace. The unit test
  (`talk_request_has_no_action_channel`) proves the request has no action channel; this proves the
  running harness never fabricates a dispatch — jointly the two are the full guarantee (plan-critic
  C-006).

## Regression
Every pre-existing `ferric-cli` test unaffected — `chat` is a new subcommand; `query`/`mcp`/`trace`/
`server`/`bench` are untouched.

## Result
`cargo test -p ferric-cli --test cli`: 32 passed (up from 28). `cargo test --workspace`: all green.
`cargo clippy --workspace --all-targets -- -D warnings`: clean (default + `backend-openai` +
`backend-mistralrs`). `cargo fmt --all --check`: clean.
