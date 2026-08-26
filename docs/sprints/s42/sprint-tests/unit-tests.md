# Sprint 42 Unit Tests

Co-located `#[cfg(test)]` in `crates/ferric-cli/src/chat.rs`. All derived from T-4202's EARS
clauses. All green (`ferric-cli` bin unit tests: 70, up from 64 — +6 chat).

## T-4202 — `chat.rs` unit tests
- `parse_chat_input_do_is_escalate` — `/do fix the bug` → `Escalate("fix the bug")`; leading/trailing
  whitespace on the request is trimmed.
- `parse_chat_input_help_exit_quit` — `/help` → `Help`; `/exit` → `Exit`; `/quit` → `Exit`.
- `parse_chat_input_empty_is_empty` — `""`, `"   "`, `"\n"` → `Empty`.
- `parse_chat_input_default_is_talk` — an ordinary line → `Talk`; an UNKNOWN `/command` (e.g.
  `/unknown`) → `Talk("/unknown")` (unknown commands are talked, never silently executed); `/do`
  with no request → `Talk("/do")`.
- **`talk_request_has_no_action_channel`** (the load-bearing structural-safety proof, ADR-052) —
  `talk_request(&history, &sampling)` returns a `CompletionRequest` with `tools.is_empty()` AND
  `constraint.is_none()`, and `validate().is_ok()` (the lawful ADR-010 "neither" case). Talk mode
  cannot carry a tool call.
- `escalation_seed_carries_history_and_protocol` — `escalation_seed(&history, &config, "chat-123")`
  produces a `ReplayedState` whose `messages` equals the full conversation, `protocol` matches the
  config, `source_session` is the chat id, and `turns == 0` (a fresh turn budget for the escalated
  run) — proving `/do` continues the SAME conversation into the constrained loop.

## Result
`cargo test -p ferric-cli` (bin unit): 70 passed (up from 64). `--features backend-openai`/
`backend-mistralrs`: the `ChatBackend::Real` variant compiles clean under both; unit tests
unaffected.
