# Plan Critique — Sprint 42

Reviewed by a foreground plan-critic that verified every reuse claim against the real source. **Core
premise confirmed:** `run_with_provider` (query.rs:718, `pub(crate)`, callable from a sibling module
as `mcp.rs` does), `build_run_config`/`RunConfig`, `create_provider` (→ `Box<dyn Provider + Send +
Sync>`), `ReplayedState` (all-pub, `Clone`), `Provider::complete` (async), `CompletionRequest`,
`Event::Note`, `JsonlSink` — all match. The security boundary is genuinely sound (the talk path
never touches `run()`/dispatch; `tools`-empty + `constraint: None` is the lawful ADR-010 "neither"
case). Two load-bearing concerns fixed before lock; three mechanical clarifications folded in.

## C-001: a fixed MockProvider script can't drive a stdin-length-driven REPL (load-bearing)
- **Finding:** `MockProvider` is a fixed queue that returns `ScriptExhausted` when drained
  (mock.rs:62-68). In a REPL the call count is driven by stdin, and talk turns (one plain-text
  completion) vs `/do` turns (the 2-item `write_file`+`task_complete` agentic script) need DIFFERENT
  completion shapes from the SAME held provider — the "mirrors query --mock" framing glosses this.
- **Response:** **fix-in-plan.** For `--mock`, build a **fresh `MockProvider` per turn**, shaped by
  the turn kind: a talk turn → `MockProvider::new(vec![text_completion(canned)])`; a `/do` turn →
  `MockProvider::new(mock_agentic_script())` (the existing `write_file`+`task_complete` shape). Fresh
  per turn ⇒ no exhaustion, and talk vs agentic completion shapes are cleanly separated. The
  session-held provider is **real-backend-only**; mock is per-turn-fresh. (Resolves C-001, C-003,
  C-004 together.)

## C-002: "one held sink + embedded run() block" is an incoherent trace shape (load-bearing)
- **Finding:** `run()` unconditionally writes a full `SessionStart` (run.rs:88) … `SessionEnd`
  (run.rs:472) envelope EVERY call — it has no "append into an open session" mode. Routing multiple
  `/do` turns through one held sink yields multiple `SessionStart`/`SessionEnd` pairs in one file,
  which breaks `replay()`'s pass-1 (aborts on the first `SessionEnd`, replay.rs:130-135) and is
  semantically contradictory. `ferric mcp` deliberately opens a FRESH file per `tools/call` for
  exactly this reason (mcp.rs:316-318).
- **Response:** **fix-in-plan — adopt the mcp precedent.** Each `/do` escalation opens its OWN fresh
  trace file (a whole coherent agentic-run trace, exactly like `ferric mcp`). Talk turns are traced
  into a SEPARATE single chat-session log file — a chat-level `SessionStart` … `SessionEnd` envelope
  written by the chat module, with one `Event::Note` per talk turn AND one `Note` per `/do`
  referencing that escalation's trace file. No `run()` envelope ever lands in the chat log, so it
  stays coherent. Both are auditable; neither doubles an envelope.

## C-003: `create_provider` is feature-gated — the default (backend-free) build needs the mock branch
- **Finding:** `create_provider` only exists under `#[cfg(any(backend-mistralrs, backend-openai))]`
  (backend.rs:78-81); the default `cargo test` build has neither. `query`/`mcp` branch on `args.mock`
  BEFORE touching `create_provider`, with a `cfg(not(any(...)))` "use --mock" fallback.
- **Response:** **fix-in-plan.** T-4202 states the branch explicitly: `if mock { fresh per-turn mock
  } else { create_provider(...) }`, with the `cfg(not(any(...)))` fallback — mirroring
  query.rs:565-566 / mcp.rs:478-535.

## C-004: the async `complete()` call needs an executor decision (block_on)
- **Finding:** `Provider::complete` is async (traits.rs:20); the CLI has no ambient tokio.
  `query`/`mcp` use `futures_executor::block_on` for mock and a held `tokio::runtime::Runtime` for a
  real backend (`mcp.rs`'s `Executor::{Mock, Real(Runtime)}`, mcp.rs:240-244).
- **Response:** **fix-in-plan.** T-4202 reuses the `Executor` pattern: hold it for the session; drive
  BOTH the talk `complete()` and the `/do` `run_with_provider` through it (`block_on` for mock,
  `runtime.block_on` for real).

## C-005: stdin-piping is a new subprocess-test pattern (feasible, minor)
- **Finding:** the existing `cli.rs` suite never pipes stdin (all `Command::...output()`); the chat
  tests need `Stdio::piped()` + `write_all` + `wait_with_output`. Combined with C-001, an
  exhausted-mock or unconsumed-`/exit` could hang the child.
- **Response:** **fix-in-plan.** T-4203 notes the new stdin-piping harness; `/exit` is handled purely
  at the parse layer (no `complete()` call), and the per-turn-fresh mock (C-001) can't exhaust — so
  an off-by-one in piped lines can't hang the child.

## C-006: the black-box non-dispatch test — sound, both legs jointly necessary
- **Finding:** the design is genuinely sound; the load-bearing proof is the UNIT test
  `talk_request_has_no_action_channel` (empty tools + `None` constraint), and the black-box test
  proves the harness doesn't fabricate a dispatch. Talk output is appended as an assistant `Message`
  and printed — never re-fed through `parse_chat_input` (no re-parse loop).
- **Response:** **reject** (not a gap) — both tests exist and are jointly necessary; recorded as a
  positive verification.

## C-007: `/do` escalation's `ReplayedState` seed needs explicit semantics
- **Finding:** `ReplayedState` is constructible (all-pub), but `run()` with a resume seed writes
  `SessionStart.resumed_from = source_session` and skips `SessionPrompt` (run.rs:108-129) — so the
  chat module must synthesize `source_session` (the chat session id), and the talk-derived history
  won't appear as a `SessionPrompt` in the escalated trace. `protocol` must equal `config.protocol`.
- **Response:** **fix-in-plan.** T-4202/ADR-052 state: `source_session` = the chat session id;
  `protocol` = `config.protocol`; `turns`/`last_text` seeded from the running history; and note the
  escalated trace has no `SessionPrompt` (the chat log carries the conversational provenance
  instead). Ties into C-002's per-`/do`-file decision.

## Confidence
proceed-with-caveats → C-001 and C-002 (load-bearing) resolved with the per-turn-fresh-mock and
per-`/do`-file-plus-chat-log designs; C-003/C-004/C-005/C-007 folded into T-4202/T-4203 as explicit
clarifications; C-006 rejected as a positive verification. Build-plan.md and test-plan.md revised
accordingly before lock.
