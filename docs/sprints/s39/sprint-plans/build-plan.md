Finalized - DO NOT EDIT

# Sprint 39 Build Plan

## Schema Tree
- Sprint Goal: session resume (`ferric query --resume <path>`)
  - Trace format extensions
    - T-3901: new event `Event::SessionPrompt` + `SessionStart.resumed_from`
    - T-3902: extend existing events for full turn fidelity (terminator `ToolCall` + `TurnEnd.truncated`)
  - Replay core
    - T-3903: `ferric-loop::replay` — reconstruct `ReplayedState` from a trace file
  - Loop wiring
    - T-3904: thread `ReplayedState` into `RunArgs`/`run()`; relax `prompt` to `Option<&str>`
  - CLI wiring
    - T-3905: `ferric query --resume <path>` CLI wiring
  - Docs
    - T-3906: ADR-049 + docs

## Execution Sequence

### T-3901: New trace event — `Event::SessionPrompt` + `SessionStart.resumed_from`
- **Touches:** `crates/ferric-trace/src/event.rs`, `crates/ferric-loop/src/run.rs`
- **Depends on:** (none)
- `Event::SessionPrompt { system: String, user: String, media: Vec<MediaPart> }` — a new, purely
  additive variant. `run()` writes it once, right after `PolicySelected`/`PromptComposed` and before
  `TurnStart(0)`, UNLESS this run IS a resume (T-3904 makes it conditional). `SessionStart` gains
  `resumed_from: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]` — an
  old `session_start` line with no `resumed_from` key still parses as `Known` with `None`).
  **(C-002, plan-critic)** `resumed_from` stores the ORIGINAL session's `session` id string (the
  same value already stamped on every `TraceEvent` line by `JsonlSink`), not a file path — paths can
  move, the id is self-describing and already present in the file itself. Resume-of-a-resume chains
  are allowed and need no special handling: `replay()` only ever reads the ONE target file named by
  `--resume`, never walks further back through a chain.
- **Success criterion (EARS):**
  - **WHEN** a session starts and is NOT a resume, **THEN** exactly one `Event::SessionPrompt`
    **SHALL** be written before `TurnStart(0)`, and `SessionStart.resumed_from` **SHALL** be `None`.
  - **WHEN** a pre-sprint-39 `session_start` trace line (no `resumed_from` key) is parsed, **THEN**
    it **SHALL** still parse as `Known` with `resumed_from: None` (backward-compat regression).

### T-3902: Extend existing events for full turn fidelity
- **Touches:** `crates/ferric-trace/src/event.rs`, `crates/ferric-loop/src/run.rs`
- **Depends on:** (none — independent of T-3901)
- Trace the terminator's (`task_complete`) `ToolCall` in ALL protocols (just don't dispatch/execute
  it — closes the `NativeTools` summary-visibility gap; `ConstrainedJson`/`TextXml` already have the
  summary in `TurnEnd.text`, so this is additionally redundant-but-harmless there, and uniform).
  **(C-003, plan-critic — the placement matters, not just "trace it somewhere")** the
  `sink.write_event(Event::ToolCall{...})` call for the terminator goes INLINE, at the exact position
  of the existing `continue` inside the per-call `for call in &actions` loop in `run.rs`'s dispatch —
  i.e. trace-order stays identical to `actions`' original (model-emission) order even when the
  terminator is mixed among other calls in the same turn (e.g. `[tool_a, task_complete, tool_b]`
  must trace in that exact order, not `[tool_a, tool_b, task_complete]`). Getting this wrong would
  silently corrupt T-3903's order-preserving replay for exactly the multi-tool-call-per-turn case.
  `TurnEnd` gains `truncated: bool` (`#[serde(default)]` for backward compat), set from
  `completion.truncated`.
- **Success criterion (EARS):**
  - **WHEN** the terminator is called in any protocol, **THEN** an `Event::ToolCall` **SHALL** be
    traced for it (matching `id`/`name`/`args`), even though it is never dispatched.
  - **WHEN** a turn's completion is truncated, **THEN** `TurnEnd.truncated` **SHALL** be `true`;
    otherwise `false`.
  - **WHEN** a pre-sprint-39 `turn_end` trace line (no `truncated` key) is parsed, **THEN** it
    **SHALL** still parse as `Known` with `truncated: false` (backward-compat regression).

### T-3903: `ferric-loop::replay` — reconstruct `ReplayedState` from a trace file
- **New file:** `crates/ferric-loop/src/replay.rs`
- **Touches:** `crates/ferric-loop/src/lib.rs` (exports), `crates/ferric-loop/src/run.rs`
- **Depends on:** T-3901, T-3902
- **(C-007, plan-critic — precision on "shared helpers")** extract EACH of the ~6 distinct
  inline `format!`/template calls in `run.rs` into its OWN small `pub(crate)` function (NOT one
  generic parameterized formatter — repetition-warn and no-progress-warn are semantically different
  wording, not interchangeable): the no-action nudge (one per protocol, existing `no_action_nudge`),
  the truncation-retry text, the repetition-guard-warned text, the no-progress-guard-warned text,
  and the failure-guard-warned text. Also make the existing private `result_message` function
  `pub(crate)` so `replay.rs` can reuse it (trivial — same crate, no visibility boundary crossed).
  `run()` and `replay()` both call these same functions so they can't drift apart.
- `pub struct ReplayedState { messages: Vec<Message>, turns: u32, last_text: Option<String>,
  protocol: ActionProtocol, source_session: String }` (`source_session` per C-002's id-not-path
  clarification above). `pub enum ReplayError { Io, Trace, MissingSessionPrompt,
  AlreadyStopped(String) }`. `pub fn replay(path: &Path) -> Result<ReplayedState, ReplayError>`:
  1. **First pass:** scan for a `SessionEnd` event anywhere in the file — if found, return
     `ReplayError::AlreadyStopped(reason)` immediately (a trace that reached ANY stop reason, clean
     or not, isn't "interrupted"). This ALSO means, by construction, any trace that survives this
     gate can contain **at most one** no-action-nudge-eligible turn and **at most one**
     truncation-eligible turn (a second occurrence of either would have produced a `SessionEnd` —
     `EmptyCompletion`/`TruncatedAction` — in the original run, which this gate already excludes).
  2. **Reconstruction pass:** walk turns via `TraceReader`, building `[system, user(+media)]` from
     `SessionPrompt`, then per turn: the assistant message (text from `TurnEnd.text`, `tool_calls`
     from that turn's ordered `ToolCall` events — now including the terminator's, per T-3902, in
     their original trace-order position); each non-terminator `ToolResult`'s reconstructed result
     message (via `result_message`, framed per the trace's own `PolicySelected.protocol`); then, if
     applicable, exactly one nudge message for that turn: a guard "warned" event → the matching
     one of the 3 guard-specific functions above, fed that turn's `ToolCall` names; zero `ToolCall`
     events (and not truncated) → the no-action nudge — **(C-004/C-006, plan-critic)** tracked via
     the SAME session-scoped one-shot semantics `run()` itself uses (`nudged_for_no_action`,
     `truncated_once` are booleans that fire ONCE per session, not a per-turn/lookahead check —
     replay mirrors this exactly, which the AlreadyStopped gate above guarantees is sufficient,
     since a second occurrence of either condition would already have been rejected); falls back to
     the generic protocol-keyed template if the exact `TextXml` parse-error text isn't recoverable
     (an accepted, narrow, explicitly-tested approximation — see test-plan.md).
  3. **A turn whose `TurnStart` has no matching `TurnEnd`** (the trace ends mid-turn — the realistic
     shape of a killed process, per **C-001, plan-critic**) is **discarded, not committed**: no
     assistant message, no `turns` increment for it, mirroring `run()`'s own behavior (it never
     pushes anything to `messages` or increments displayed progress for a turn until `TurnEnd`
     arrives). The continued run simply re-attempts that turn number fresh.
- **Success criterion (EARS):**
  - **WHEN** a trace has no `SessionPrompt` event, **THEN** `replay` **SHALL** return
    `ReplayError::MissingSessionPrompt`.
  - **WHEN** a trace already contains a `SessionEnd` event, **THEN** `replay` **SHALL** return
    `ReplayError::AlreadyStopped(reason)` rather than reconstruct a "continuable" state.
  - **WHEN** a trace has a `SessionPrompt`, a `PolicySelected`, and N complete turns with no
    `SessionEnd`, **THEN** `replay` **SHALL** return a `ReplayedState` whose `messages` matches
    exactly what `run()`'s own in-memory state would have held at that point, `turns` **SHALL**
    equal the number of turns that have a matching `TurnEnd`, and `protocol` **SHALL** equal the
    trace's `PolicySelected.protocol`.
  - **WHEN** a trace's LAST `TurnStart` has no matching `TurnEnd` (interrupted mid-turn), **THEN**
    `replay` **SHALL** discard that dangling turn entirely (not increment `turns`, not append a
    partial assistant message for it).
  - **WHEN** a turn's `TurnEnd.truncated` is `true`, **THEN** `replay` **SHALL** append the same
    truncation-retry nudge `run()` would have, without treating that turn's partial text as an
    executable action.
  - **WHEN** a `TextXml` turn's no-action nudge cannot recover the exact original parse-error text,
    **THEN** `replay` **SHALL** still produce a valid, non-empty nudge message (the generic
    protocol-keyed template), never panicking or returning an error for this case.

### T-3904: Thread `ReplayedState` into `RunArgs`/`run()`; relax `prompt` to `Option<&str>`
- **Touches:** `crates/ferric-loop/src/run.rs`, `crates/ferric-cli/src/query.rs` (`run_with_provider`/
  `drive_mock`/`drive_real` pass-through), `crates/ferric-cli/src/mcp.rs` (`run_one` pass-through,
  unaffected in behavior — always passes `Some`)
- **Depends on:** T-3903
- `RunArgs` gains `resume: Option<ReplayedState>`. `run()`'s `prompt` parameter becomes
  `Option<&str>` (mechanical ripple through `run_with_provider`/`drive_mock`/`drive_real`/`run_one` —
  every existing call site wraps its existing `&str` in `Some(...)`, byte-identical — confirmed
  against `mcp.rs`'s `run_one`, whose one call site always has a real prompt from the required MCP
  `tools/call` argument, so it always passes `Some`). When `args.resume` is `Some`, `run()` seeds
  `messages` from `replayed.messages` (plus one appended `Message::user(p)` if `prompt` is ALSO
  `Some(p)`), `turns` from `replayed.turns`, `last_text` from `replayed.last_text`; writes
  `SessionStart.resumed_from = Some(replayed.source_session)`; does NOT write a new `SessionPrompt`.
  When `args.resume` is `None`, behavior is byte-identical to today, EXCEPT `prompt: None` with no
  resume is now a defensive error return (not a panic) — a state the CLI layer (T-3905) is
  responsible for never producing.
  **(C-009, plan-critic)** `PolicySelected` (and `PromptComposed`, if `args.prompt_lineage` is
  `Some`) are still written fresh on EVERY run, resumed or not — they record the tier/protocol/
  budgets this specific continuation actually runs under, which may differ slightly from the
  original session's if different flags are passed (harmless: only affects the budget ceiling going
  forward, not correctness — see T-3905's protocol-match validation for the one field that WOULD be
  correctness-breaking if it differed). The system message itself, however, is frozen from
  `replayed.messages[0]` — `args.system_prompt`/`args.prompt_lineage` (i.e. `--prompts-dir`/
  `Animus.md`) are **silently inert for the system message on a resumed run**; this is documented
  explicitly (not left implicit) in T-3905's CLI-layer behavior and ADR-049 (T-3906).
- **Success criterion (EARS):**
  - **WHEN** `RunArgs.resume` is `None` and `prompt` is `Some`, **THEN** `run()`'s behavior **SHALL**
    be byte-identical to before this sprint (regression — every pre-existing `ferric-loop` test
    keeps passing unchanged).
  - **WHEN** `RunArgs.resume` is `Some(replayed)`, **THEN** `run()` **SHALL** seed
    `messages`/`turns`/`last_text` from it, write `resumed_from`, and skip writing a new
    `SessionPrompt`.
  - **WHEN** `RunArgs.resume` is `None` and `prompt` is `None`, **THEN** `run()` **SHALL** return an
    error rather than panic.

### T-3905: `ferric query --resume <path>` CLI wiring
- **Touches:** `crates/ferric-cli/src/query.rs`
- **Depends on:** T-3904
- `QueryArgs` gains `resume: Option<PathBuf>`; `prompt` becomes `Option<String>` (required UNLESS
  `--resume` is given — clap `required_unless_present`). `run_query`, when `--resume` is given:
  calls `ferric_loop::replay(path)`; on `AlreadyStopped`/`MissingSessionPrompt`/io error, prints a
  clear message and exits `FAILURE` without attempting to run; validates the replayed `protocol`
  matches this invocation's resolved `config.protocol` (mismatch → clear error naming both, exit
  `FAILURE`); passes `Some(replayed)` into `RunArgs.resume`. **(C-009, plan-critic)** when
  `--resume` is given AND `--prompts-dir`/`FERRIC_PROMPTS_DIR` was resolved OR an `Animus.md` exists
  at the workspace root, prints a stderr note that these are ignored for a resumed run's system
  message (frozen from the replayed trace) — cheap, prevents a silently-confused user expecting an
  edited `Animus.md` to apply to a continuation.
- **Success criterion (EARS):**
  - **WHEN** `--resume <path>` is given with no prompt argument, **THEN** `ferric query` **SHALL**
    replay `<path>`, continue with no new user message, and exit per the continued run's own
    `StopReason` (today's convention).
  - **WHEN** `--resume <path>` is given ALONGSIDE a prompt argument, **THEN** the prompt **SHALL** be
    appended as one extra user message after the replayed history.
  - **WHEN** neither `--resume` nor a prompt argument is given, **THEN** `ferric query` **SHALL**
    fail with a usage error (regression — matches today's "prompt required" behavior when `--resume`
    is never used).
  - **WHEN** `--resume <path>`'s recorded protocol doesn't match this invocation's resolved protocol,
    **THEN** `ferric query` **SHALL** fail with a clear error naming both, without running.
  - **WHEN** `--resume <path>` names an already-`SessionEnd`ed trace, **THEN** `ferric query`
    **SHALL** fail with a clear error naming the original stop reason, without running.
  - **WHEN** `--resume <path>` is combined with `--prompts-dir`/an `Animus.md`-bearing workspace,
    **THEN** `ferric query` **SHALL** print a stderr note that both are ignored for the system
    message (never silent about a flag that has no effect).

### T-3906: ADR-049 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-3901, T-3902, T-3903, T-3904, T-3905
- ADR-049: the resume-scope decision (interrupted-task-only, not chat-continuation) and why; the two
  new/extended trace events + the `NativeTools`-summary-gap fix's independent value; the
  fresh-guards decision; the protocol-match validation; the `TextXml` parse-error approximation; the
  system-prompt-frozen-on-resume behavior (C-009); **(C-011, plan-critic)** one explicit sentence
  naming the tension the "`--resume` + an extra prompt" affordance sits in: it's structurally the
  same mechanism as the REJECTED use-case-2 (follow-up-on-completed-task), but is confined to
  genuinely-still-incomplete traces only (the `AlreadyStopped` gate is the real ADR-011 boundary,
  not the mere absence of an extra-prompt flag) — named explicitly rather than glossed over, per the
  project's own precedent of naming tensions (e.g. ADR-047's explicit scope-bounding list); the
  deferred compaction/`--save-interval` follow-on (sprint 40) with the user's own rationale for
  splitting it out; `ferric mcp --resume` explicitly deferred (launch-time-fixed design doesn't
  naturally support per-call trace selection).
- **Success criterion (EARS):**
  - **WHEN** ADR-049 is read, **THEN** it **SHALL** state the resume-scope decision, the trace-format
    changes + rationale, the fresh-guards decision, the system-prompt-frozen-on-resume behavior, the
    use-case-2 tension acknowledgment, and explicitly list what's deferred (compaction/sprint 40, MCP
    resume).
