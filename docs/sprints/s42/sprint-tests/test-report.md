# Sprint 42 Test Report — raw chat mode (`ferric chat`)

## Summary
`ferric chat` — the hybrid talk+escalate REPL — is covered by 6 co-located unit tests + 4 CLI
subprocess tests, all derived from T-4202's EARS clauses. A foreground test-critic **independently
reproduced the security boundary against the actual built binary** in both directions and returned
**clean**. The one edge it raised (an escalation-filename collision on sub-millisecond `/do` bursts)
was fixed inline rather than deferred, since scripted/piped chat makes it real.

## The crux: the security boundary holds for real
The entire point of this sprint is a safe boundary between an unconstrained talk path and the
guarded action path. It is proven, not merely asserted:
- **Unit** (`talk_request_has_no_action_channel`): the talk request has `tools.is_empty()` AND
  `constraint.is_none()` (the lawful ADR-010 "neither" case) — structurally no action channel.
- **Black-box** (`chat_talk_turn_is_not_dispatched`): a talk line that LOOKS like a tool call
  (`write_file to /etc/passwd`) opens no escalation trace, writes no `tool_call`/`tool_result`/
  `permission_check` anywhere, and never touches the workspace.
- **Independently re-run by the critic** against `target/debug/ferric`: the adversarial talk line
  dispatched nothing and wrote nothing; `/do write a file` drove the full guarded loop (a real
  `permission_check` + `tool_result` + a workspace write). The `Talk` branch calls only
  `provider.complete()`, never `run_with_provider`/dispatch, and talk output is never re-fed through
  `parse_chat_input` — so no model self-escalation path exists (ADR-005 upheld).

## Coverage by task
- **T-4201** (ADR-052): docs — verified by read (the built code matches the ADR exactly).
- **T-4202** (`chat.rs` REPL): 6 unit tests — `parse_chat_input` (4 arms incl. unknown-command →
  Talk), `talk_request_has_no_action_channel`, `escalation_seed_carries_history_and_protocol`.
- **T-4203** (CLI subprocess): 4 stdin-piped tests — talk-then-exit, `/do` escalation (separate
  agentic trace + workspace write), `/help`, and the black-box non-dispatch proof.
- **T-4204** (docs): README + `main.rs` surface doc + agent-tasks wrap-up — verified by read.

## Critic finding and resolution
- **C-001** (fix-code, applied): `/do` trace files named `chat-esc-{now_ms()}.jsonl` could collide
  on two escalations in the same millisecond (real for scripted chat). Added a per-session monotonic
  `esc_count` suffix (`chat-esc-{ms}-{n}.jsonl`). Not a security-boundary edge; a trace-integrity
  hardening. Re-verified green.

## Final verification
- `cargo test --workspace`: all green (`ferric-cli` bin unit 70, `ferric-cli` cli 32, plus all other
  suites unaffected).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (default + `backend-openai` +
  `backend-mistralrs`).
- `cargo fmt --all --check`: clean.
- The `ChatBackend::Real` variant compiles clean under both backend feature sets.

## Confidence
Clean — proceed to Loop Phase.
