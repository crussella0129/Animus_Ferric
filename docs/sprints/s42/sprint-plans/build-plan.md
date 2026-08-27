Finalized - DO NOT EDIT

# Sprint 42 Build Plan

## Schema Tree
- Sprint Goal: `ferric chat` — a hybrid talk+escalate REPL (the ADR-011-revision chat mode)
  - Decision record
    - T-4201: ADR-052 — the chat security boundary
  - The REPL
    - T-4202: `crates/ferric-cli/src/chat.rs` + wire into `main.rs`
  - Tests
    - T-4203: unit (parse/talk-request/escalation) + CLI subprocess
  - Docs
    - T-4204: README + main.rs surface doc + agent-tasks wrap-up

## Execution Sequence

### T-4201: ADR-052 — the chat security boundary
- **Touches:** `decisions.md`
- **Depends on:** (none)
- Records the hybrid decision and the boundary: talk mode is the harness's FIRST unconstrained-
  completion path, structurally safe (empty `tools` + `constraint: None` + output text-only + never
  dispatched — safety is structural, not a prompt); escalation is USER-initiated only (ADR-005 — a
  model-initiated escalation would consult the LLM on a security decision, forbidden); escalated
  turns reuse the constrained `run_with_provider`/`ferric-guard`/guards/trace path unchanged; the
  talk path lives in the CLI chat module while `ferric-loop::run()` stays always-constrained;
  launch-time-fixed containment (ADR-046). **Trace shape (plan-critic C-002, adopting the `ferric
  mcp` precedent):** each `/do` escalation opens its OWN fresh agentic trace file (a whole coherent
  `run()` `SessionStart..SessionEnd` trace, exactly like `ferric mcp` opens one file per
  `tools/call`); talk turns are traced into a SEPARATE single chat-session log file (a chat-level
  `SessionStart..SessionEnd` envelope with a `Note` per talk turn + a `Note` per `/do` referencing
  its escalation file). No `run()` envelope is ever nested into another. **Escalation-seed semantics
  (plan-critic C-007):** the `/do` `ReplayedState` seed uses `source_session` = the chat session id,
  `protocol` = `config.protocol`, `turns`/`last_text` from the running history; the escalated trace
  has no `SessionPrompt` (the chat log carries the conversational provenance). Explicit deferrals:
  TUI, talk-mode streaming, a dedicated `ChatTurn` trace event, Animus-IDE wiring.
- **Success criterion (EARS):**
  - **WHEN** ADR-052 is read, **THEN** it **SHALL** state the talk-mode structural-safety guarantee,
    the user-initiated-escalation rule (ADR-005) and why model-initiated escalation is forbidden,
    the reuse of the unchanged constrained loop for escalated turns, and the explicit deferrals.

### T-4202: `crates/ferric-cli/src/chat.rs` — the hybrid REPL + wire into `main.rs`
- **Touches:** new file `crates/ferric-cli/src/chat.rs`; `crates/ferric-cli/src/main.rs`
- **Depends on:** T-4201
- `ChatArgs` (mirrors `QueryArgs` minus `prompt`/`resume` — launch-time-fixed containment).
  `run_chat`: workspace + `config::load_layered` + `build_run_config`; a chat-session log
  `JsonlSink` (chat-level `SessionStart` written at open, `SessionEnd` at exit); history
  `Vec<Message>` seeded from `config.system_prompt`; a REPL over stdin lines (plain `BufRead::lines`,
  no TUI). Pure `parse_chat_input(&str) -> ChatInput` and `talk_request(&[Message],
  &SamplingParams) -> CompletionRequest`.
- **Provider + executor split (plan-critic C-001/C-003/C-004):** branch on `args.mock` BEFORE
  touching `create_provider` (which is `#[cfg(any(backend-mistralrs, backend-openai))]`-gated —
  mirror `query.rs:565` / `mcp.rs`'s `Executor::{Mock, Real(Runtime)}` + the `cfg(not(any(...)))`
  "built without backends; use --mock" fallback). **Real backend:** hold ONE `create_provider`
  `Box<dyn Provider>` + ONE `tokio::runtime::Runtime` for the session; drive both talk `complete()`
  and `/do` `run_with_provider` via `runtime.block_on`. **Mock:** build a FRESH `MockProvider` PER
  TURN, shaped by turn kind — a talk turn → `MockProvider::new(vec![text_completion(canned)])`; a
  `/do` turn → `MockProvider::new(<the write_file+task_complete agentic script>)` — driven via
  `futures_executor::block_on`. Fresh-per-turn ⇒ no `ScriptExhausted` in the REPL, and talk vs
  agentic completion shapes are cleanly separated.
- **Per-turn behavior:** Talk turns → `provider.complete(talk_request(...))` (empty tools, `None`
  constraint), print the response, append `Message::assistant(resp)` to history, write a `Note` to
  the chat-session log. `/do` turns → open a FRESH per-escalation `JsonlSink`, build a
  `ReplayedState` from the running history (`source_session` = chat session id, `protocol` =
  `config.protocol`, `turns`/`last_text` from history), call `run_with_provider(..., prompt:
  Some(text), resume: Some(state), sink: &mut escalation_sink, ...)`, fold the outcome's final text
  into history, and write a referencing `Note` to the chat-session log. `Command::Chat(Box<ChatArgs>)`
  wired into `main.rs`.
- **Success criterion (EARS):**
  - **WHEN** `parse_chat_input` is given a `/do <text>` line, **THEN** it **SHALL** return
    `Escalate(text)`; `/help` → `Help`; `/exit` or `/quit` → `Exit`; an empty/whitespace line →
    `Empty`; any other line → `Talk(line)`.
  - **WHEN** `talk_request` builds a talk completion, **THEN** the request's `tools` **SHALL** be
    empty AND `constraint` **SHALL** be `None` (structural safety — talk mode has no action channel).
  - **WHEN** a `Talk` turn runs, **THEN** the model output **SHALL** be printed and appended to
    history as an assistant message, and **SHALL NOT** be parsed for or routed to tool dispatch.
  - **WHEN** an `Escalate` turn runs, **THEN** it **SHALL** drive the existing constrained
    `run_with_provider` with the conversation history as the `resume` seed (the same guarded/traced
    path as `ferric query`).

### T-4203: tests
- **Touches:** `crates/ferric-cli/src/chat.rs` (unit `#[cfg(test)]`), `crates/ferric-cli/tests/cli.rs`
- **Depends on:** T-4202
- Unit: `parse_chat_input` for every arm; `talk_request` empty tools + `None` constraint (the
  structural-safety proof); the escalation helper builds a `ReplayedState` carrying prior history.
  CLI subprocess (`ferric chat --mock`, stdin piped via the test harness): a talk line + `/exit`
  prints a response and writes one session trace; a `/do <req>` line drives the mock agentic loop
  (agentic events — `tool_call`/`session_end` — appear in the trace); `/help` lists commands.
- **Success criterion (EARS):**
  - **WHEN** the unit + CLI tests run, **THEN** every EARS clause in T-4202 **SHALL** have a
    corresponding passing test, including the talk-mode-no-action-channel structural proof and the
    escalation-reuses-the-guarded-loop proof.

### T-4204: docs
- **Touches:** `README.md`, `crates/ferric-cli/src/main.rs` (module doc), `agent-tasks/agent-tasks.md`,
  `agent-tasks/completed-tasks.md`
- **Depends on:** T-4201–T-4203
- README Status bump + a Sprint 42 timeline entry; `main.rs`'s surface doc updated (chat mode is no
  longer "future/unbuilt"); the sprint 42 backlog section rewritten in-progress → completed summary
  (matching sprints 38–41's precedent), the ADR-011-revision chat item marked built.
- **Success criterion (EARS):**
  - **WHEN** README's Sprint 42 entry and `main.rs`'s surface doc are read, **THEN** both **SHALL**
    describe `ferric chat` as built, with the hybrid talk/escalate boundary and an ADR-052 reference.
