# Test Critique — Sprint 42

Reviewed by a foreground test-critic that **independently reproduced the security boundary against
the actual built binary** (`target/debug/ferric`) in both directions, verified the code paths, and
re-ran the full suite. **Confidence: clean.** One minor flake-risk edge (fixed, below).

## The crux — security boundary independently reproduced (critic, against the built binary)
- **Adversarial talk line** (`write_file to /tmp/evil now`): NO `chat-esc-*.jsonl`; the chat-session
  log is `session_start`/`note`/`session_end` only (no `tool_call`/`tool_result`/`permission_check`);
  NO workspace file written. ✅
- **`/do write a file`**: exactly one `chat-esc-*.jsonl` with the full guarded loop
  (`tool_call`/`permission_check`/`tool_result`/`session_end`); `ferric-mock.txt` written. ✅
- **Code-path review:** talk output is only `println!` + `Message::assistant` — never re-fed through
  `parse_chat_input`, so there is no model self-escalation path (ADR-005 upheld). ADR-052 matches
  the built code exactly. The two structural-safety tests (unit `talk_request_has_no_action_channel`
  + black-box `chat_talk_turn_is_not_dispatched`) are jointly sufficient — the request shape AND the
  dispatch path are both pinned.

## C-001: escalation trace filename can collide on sub-millisecond `/do` bursts
- **Finding:** each `/do` names its trace file `chat-esc-{now_ms()}.jsonl` (millisecond
  granularity). Two `/do` turns within the same millisecond — implausible for human typing, but
  possible for SCRIPTED/piped chat — would collide on the same path. Not a security-boundary edge
  (talk-vs-esc prefixes never clash; this is esc-vs-esc only); the critic suggested defer.
- **Response:** **fix-code (applied)** — cheap and correct, and scripted chat makes it more than
  hypothetical. Added a per-session monotonic `esc_count` suffix: `chat-esc-{ms}-{n}.jsonl`. A new
  unit-adjacent guarantee; the existing `chat_do_escalates_to_agentic_loop` test still matches
  (`chat-esc-` prefix). Re-verified green.

## Screened, no concern (critic's independent checks)
- No fabricated validation — all counts (70 bin-unit / 32 cli) and both boundary claims hold when
  re-run.
- Structural-safety genuinely proven, not asserted; the `Talk` branch calls only `backend.talk()` →
  `provider.complete(talk_request)`, never `run_with_provider`/dispatch.
- No self-escalation path; `chat_do_escalates` genuinely distinguishes a real escalation (requires
  an esc file + `tool_call` + the workspace write) from a talk turn.
- EARS coverage complete for T-4201/T-4202; no flake/hang (suite 4.19s; `/exit` parse-layer-only,
  fresh-per-turn mock); the canned mock is adequate (the guarantee is request-shape + dispatch-path,
  both model-text-independent).

## Confidence
clean → C-001 fixed (not just deferred, since scripted chat makes the collision real); the security
guarantee holds for real against the built binary in both directions.
