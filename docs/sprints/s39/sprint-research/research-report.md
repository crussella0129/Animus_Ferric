# Sprint 39 Research Report

## Decisions Reviewed
- **2026-06-10 ADR-002 — JSONL trajectory is the source of truth** (sprint 0) — relevance: directly
  governs this sprint. The trace grows additively; readers tolerate unknown event types so old
  binaries keep reading new traces. Any new event this sprint adds must follow that discipline. Full
  reconstruction of the in-memory turn loop (see §4) requires at least ONE new, purely-additive event
  type — consistent with, not a revision of, ADR-002.
- **2026-06-11 ADR-011 — no chat/REPL catch-all** (sprint 1) — relevance: `ferric query` stays a
  one-shot, workspace-scoped, bounded task. This sprint must NOT turn `--resume` into an open-ended
  chat continuation mechanism — see §5's use-case framing.
- **2026-06-23 ADR-029 — persisted `ModelProfile` read-back, restrict-only** (sprint 2/8-era,
  formalized later) — relevance: precedent for "durable state read once at launch, from a file the
  user points at or a well-known path" — the shape `--resume <path>` should follow.
- **2026-07-03 ADR-047 — streaming inference, `RunArgs` gains one new `Option` field** (sprint 37) —
  relevance: precedent for extending `RunArgs`/`run()` with a new optional capability
  (`stream_sink: Option<&dyn Fn(...)>`) that is `None` (byte-identical) for every existing caller.
  Resume's reconstructed-history injection point should follow the same non-invasive pattern.
- **2026-07-04 ADR-048 — persistent config precedence** (sprint 38) — relevance: `--resume <path>`
  names a SPECIFIC file for ONE invocation; it is not a good fit for the bounded `Config` field list
  (unlike `--workspace`/`prompt`, which are also not config-surfaced). No revision proposed — this
  sprint's new flags stay CLI-only.

No prior decision is being violated; ADR-002 gains new event types (additive), not a redefinition.

## 1. Sprint Goal
Add "session resume" to `ferric query`: given a prior session's JSONL trace file, reconstruct
enough of the original turn-loop state (system/user prompt, assistant turns, tool calls and
results) to continue working in that same context, without needing the process to have stayed
alive. The backlog's originally-scoped shape is `--resume <path>` (the trace to resume from) plus
`--save-interval` (a periodic-persistence knob) — user-chosen 2026-07-04 as this sprint's focus
from a shortlist that also included the chat-mode ADR and MCP streaming notifications.

## 2. Existing Code Survey
| File | Relevance | Notes |
|------|-----------|-------|
| `decisions.md` (ADR-002 entry) | high | The trace is the source of truth; additive-only event growth; readers tolerate unknown types. |
| `crates/ferric-trace/src/event.rs` | high | Full `Event` vocabulary. `TurnEnd` carries the assistant's raw completion `text` (not a summary) and a tool-call **count**, not the calls themselves. No event stores the original system/user prompt text, media, or a nudge message's literal wording. |
| `crates/ferric-trace/src/reader.rs` | high | `TraceReader` is a plain `Iterator<Item = Result<TraceRecord, FerricError>>`; `ParsedEvent::{Known,Unknown}` already gives forward/backward tolerance for free — no reader change needed for replay to work against future trace versions. |
| `crates/ferric-trace/src/sink.rs` | high | **`JsonlSink::open` always initializes `next_seq: 0`**, even when opening (append-mode) an existing non-empty file. Reusing an existing trace file for a resumed session's NEW events would silently restart `seq` at 0 and collide with the file's existing sequence numbers. This is a real footgun if "continue writing into the same file" were chosen (see §4). |
| `crates/ferric-loop/src/run.rs` | high | The actual in-memory state to reconstruct: `messages: Vec<Message>` (system, user, then per-turn assistant/tool-result/nudge messages), `last_text`, `turns`, and three guards' internal history (`RepetitionGuard`/`ProgressGuard`/`FailureGuard`). Also: the terminator (`task_complete`) call is deliberately **never** `ToolCall`-traced (dispatch `continue`s past it) — for `NativeTools` specifically, this means the terminator's `args` (the actual summary) is not recorded ANYWHERE in the trace, since native `text` is `None` for a pure-tool-call completion. `ConstrainedJson`/`TextXml` don't have this gap — their raw `TurnEnd.text` already contains the full action including the summary. |
| `crates/ferric-core/src/message.rs` | high | `Message`/`ToolCall` are small and fully `Serialize`/`Deserialize` already — no blocker to persisting/reconstructing them. |
| `crates/ferric-core/src/scale.rs` (tier table) | medium | `max_turns` ranges 15 (Nano) to 80 (Ultra) per tier — sessions are small and bounded by design (ADR-028's ring ceilings compound this). This matters directly for scoping `--save-interval` (see §4/§5): full-replay-from-scratch of a bounded trace is cheap, so a periodic-checkpoint optimization may not be load-bearing at this project's actual scale. |
| `crates/ferric-cli/src/query.rs` | high | `run_query`'s existing shape: builds `messages` implicitly inside `ferric_loop::run()`, not in `query.rs` itself — a resume feature's reconstructed history has to be threaded INTO `run()` (a new `RunArgs` field), not built ad hoc in the CLI layer, or the loop's guard/dispatch logic would have to be duplicated. |
| `crates/ferric-cli/src/mcp.rs` | low | `McpServer`'s launch-time-fixed design (ADR-046) doesn't naturally support "resume a specific prior session" per `tools/call` — likely out of scope for MCP this sprint (see §5 deferrals). |
| `agent-tasks/agent-tasks.md` (Production-Readiness Roadmap section) | high | The original backlog bullet: "replay the existing JSONL trajectory (ADR-002) to reconstruct loop state; `--resume <path>` + `--save-interval`. The reader's unknown-event tolerance already gives forward compat." — the starting scope for this sprint. |
| `crates/ferric-cli/tests/cli.rs` | medium | Existing trace-inspection test patterns (`policy_tier`, offered-tools extraction) — the idiom this sprint's new tests should follow. |
| `crates/ferric-loop/src/repetition.rs`, `progress.rs`, `failure.rs` | medium | The three guards' internal state is turn-scoped history (last N actions/results). For a resumed session, starting these guards FRESH (rather than replaying their exact internal state) is almost certainly correct — see §5. |

## 3. External Sources
None consulted. This sprint is a self-contained internal architecture question (how to replay
this project's own JSONL format into its own in-memory loop state) — it does not hinge on any
external library, API, or vendor documentation the way, e.g., sprint 37's SSE streaming design did.

## 4. Risks, Unknowns, Dependencies
- **Risk — the trace format today cannot losslessly reconstruct the original conversation.** Two
  concrete gaps, both closeable with small, additive trace changes:
  1. The original **system + user prompt text** (and any attached media) is never recorded as
     literal text — only derived metadata (`PromptComposed`'s lineage ids/versions,
     `PromptAssembled`'s char/message counts). Fix: one new, additive event (e.g.
     `Event::SessionPrompt { system: String, user: String, media: Vec<MediaPart> }`), written once
     per session right after `PolicySelected`/`PromptComposed`, before `TurnStart(0)`.
  2. For **`NativeTools` specifically**, a `task_complete` call's `args` (the summary) is never
     traced — dispatch's loop intentionally `continue`s past the terminator without writing a
     `ToolCall` event for it, and native `text` is `None` for a pure-tool-call completion. Fix:
     trace the terminator call too (just don't dispatch/execute it) — a small, valuable fix
     independent of resume (today a `NativeTools` session's trace literally cannot show what
     summary the model gave, a real audit gap on its own).
  Everything else in the per-turn message history (assistant text for `ConstrainedJson`/`TextXml`,
  every non-terminator tool call + its full result, and the various guard-triggered nudge messages)
  IS already fully reconstructable from existing events — nudge text is either a static
  protocol-keyed template (`no_action_nudge`, the truncation-retry message) or is deterministically
  derivable from the turn's already-traced `ToolCall` events (the "you already called X" wording
  only needs the repeated tool names, which are exactly what that turn's `ToolCall` events name) —
  no new field needed for those. The one narrow miss: a `TextXml` parse-error's exact message text
  isn't traced (only that a no-action nudge fired) — low-priority, a generic fallback message is an
  acceptable approximation for that one edge case.
- **Risk — reusing an existing trace file's `JsonlSink` for the resumed session's new events would
  collide sequence numbers** (`next_seq` always starts at 0 regardless of what's already on disk).
  Recommend NOT reusing the old file: start a brand-new trace file/session for the continuation (as
  every invocation already does today), linked back via a new field on `SessionStart` (e.g.
  `resumed_from: Option<String>`, the prior session's id or trace path) — this also preserves
  ADR-002's implicit "one immutable, append-only file per session" invariant; a resumed session's
  history is never rewritten or reused, only read.
- **Unknown — which of two real use cases "session resume" means**, and they lead to different
  designs:
  1. **Resume an interrupted, still-incomplete task** (the process crashed/was killed mid-loop,
     before reaching any `StopReason`): replay history, then CONTINUE calling the provider for more
     turns on the SAME original task — no new user-supplied prompt needed. This is the more literal
     reading of "session resume" (recovering interrupted work) and pairs naturally with
     `--save-interval` (a knob relevant to how much progress could be lost if the process dies again
     mid-continuation) and with ADR-011 (it's still one bounded task, not a chat).
  2. **Follow up on an already-completed task** (the prior session reached `FinalText`/
     `TaskComplete`/etc.): replay history, append a NEW user-supplied prompt, run a fresh bounded
     loop (fresh turn budget, fresh guards). This is closer to a chat-continuation UX and sits
     closer to (without crossing) the line ADR-011 draws against a REPL/chat catch-all.
  Both are legitimate, and a real product could eventually want both — but the flag surface differs
  (case 1: `prompt` becomes optional when `--resume` is given; case 2: `prompt` stays required as
  the follow-up). This is a genuine scope decision for the Plan Phase, not something the research
  survey can resolve by reading more code — recommend surfacing it explicitly (see §5).
- **Unknown — what `--save-interval` is actually for at this project's scale.** `JsonlSink` already
  flushes every single event immediately (crash-durable up to the last completed event with no
  interval config), and sessions are bounded (15–80 turns per tier) — cheap to fully replay from
  scratch every time. A periodic-checkpoint optimization (skip replaying from turn 0 by snapshotting
  reconstructed state every N turns) is a real, well-understood pattern in other systems, but may be
  solving a problem this codebase doesn't actually have yet at its typical scale. Recommend treating
  this as a genuinely open scope question for the Plan Phase rather than guessing a design for a
  knob whose necessity isn't demonstrated by the codebase's realistic session sizes.
- **Dependency:** none new — `Message`/`ToolCall`/`Event` are all already `Serialize`/`Deserialize`;
  `TraceReader` already exists and needs no changes for backward/forward tolerance (ADR-002's whole
  point). No new crate dependency anticipated.

## 5. Recommended Approach
**Primary:** scope this sprint to use case 1 above — **resume an interrupted, still-incomplete
task** — since it's the more literal, more clearly ADR-011-compatible reading of "session resume,"
and it's the one `--save-interval` most plausibly pairs with. Concretely:
1. Add `Event::SessionPrompt { system, user, media }` (closes the biggest reconstruction gap) and
   trace the terminator's `ToolCall` too (closes the `NativeTools`-summary gap) — both small,
   additive, ADR-002-consistent trace changes with real value independent of resume.
2. Add `SessionStart.resumed_from: Option<String>` (additive, `#[serde(default)]`-safe) linking a
   resumed session back to its source trace, without ever reusing/rewriting the old file (sidesteps
   the `JsonlSink::open`'s `next_seq`-restart footgun entirely).
3. A new `ferric-loop` function, e.g. `replay(path) -> Result<ReplayedState, ReplayError>`
   (`ReplayedState { messages: Vec<Message>, turns: u32, last_text: Option<String> }`), built on
   `TraceReader`, reusing the SAME nudge-text-formatting logic `run()` already has (extract it into
   small shared helpers so the live loop and the replay path can't drift apart) — mirrors ADR-047's
   pattern of a small, additive, well-isolated new capability.
4. `RunArgs` gains one new field (e.g. `resume: Option<ReplayedState>`) — `None` for every existing
   caller (byte-identical), `Some` skips constructing the initial `[system, user]` messages and
   seeds `turns`/`last_text` instead, then the turn loop proceeds exactly as today.
5. `ferric query --resume <path>`: `prompt` becomes optional (clap: make it `Option<String>` and
   validate "required unless `--resume` is given" at the CLI layer, similar to existing
   backend-specific required-field validation); guards (`RepetitionGuard`/`ProgressGuard`/
   `FailureGuard`) start fresh for the resumed run — replaying their exact internal turn-history
   isn't necessary for correctness (they exist to catch NEW flailing, not re-litigate old turns) and
   would add real complexity for no clear benefit.
6. `--save-interval`: **flag as an open Plan-Phase/user decision**, not something this report
   resolves — options range from "don't build it this sprint, full-replay is cheap enough at this
   project's scale" to "a periodic checkpoint event, reusing the same additive-trace-event pattern."

**Alternative considered:** use case 2 (follow-up-on-completed-task, closer to a chat continuation).
Rejected as the PRIMARY scope (though not incompatible with building on top of the same replay
machinery later) because it sits closer to the line ADR-011 draws, and because the backlog's own
pairing with `--save-interval` reads more naturally as crash-recovery than as chat continuation.

**Rationale:** the interrupted-task framing is the smaller, more clearly-scoped, more clearly
ADR-011-compatible slice; it reuses existing loop machinery (guards, dispatch, protocol handling)
completely unchanged, touching only the initial-state construction; and it surfaces exactly two new
trace fields, both independently valuable (the `NativeTools` terminator-summary gap is a real,
pre-existing audit gap this sprint's research incidentally found, not something invented to serve
resume). The genuinely open questions (use-case scope, `--save-interval`'s necessity) are called
out explicitly rather than guessed at, matching this project's "ask, don't pick" stop criterion for
real product ambiguity.

## Scope Decided (user, 2026-07-04, mid-research)
Two follow-up questions were put to the user before finalizing scope, since the answers materially
change what this sprint builds:
1. **Resume use case: "resume an interrupted task" (recommended option) — confirmed.** Sprint 39
   builds exactly §5's primary recommendation: `--resume <path>` replays a prior session's trace and
   continues the SAME still-incomplete task with more turns; no new prompt required.
2. **`--save-interval`'s actual purpose, reframed by the user beyond this report's two guesses:**
   "Perhaps we need to build session compaction into this, based on the size of the context window
   of the model being used and how much context is left." This is a genuine, separate, and
   significant feature this report had not surveyed: **`RunPolicy.prompt_budget_tokens` (70% of
   `ModelProfile.ctx`, capped) is already computed and traced (`PolicySelected`) but is never
   actually enforced anywhere in `run.rs`** — nothing today prevents `messages` from growing past
   the model's real context window over a long session. A follow-up round confirmed:
   - **Compaction strategy: model-driven summarization (recommended option) — confirmed.** A
     dedicated single-shot, no-tools summarizer condenses older turns into one synthetic
     "progress so far" message as the budget nears — architecturally the SAME mechanism pattern as
     `ferric-research::summarize_quarantined` (a constrained, tools-empty single completion), but
     NOT a literal reuse of that function: Ornstein's summarizer is specifically shaped for
     **untrusted** external content (quarantine framing, `untrusted: true` provenance stamping);
     compaction summarizes the agent's own **trusted** turn history, a different trust tier with no
     injection-containment need. A new, purpose-built summarizer following the same
     constrained/no-tools shape is the right move, not repurposing Ornstein's.
   - **Sprint split: compaction becomes its OWN dedicated sprint 40 (recommended option) —
     confirmed.** Sprint 39 stays narrowly scoped to `--resume` alone; `--save-interval` (in ANY
     form — periodic trace checkpoint or budget-driven compaction trigger) is dropped from sprint
     39's scope entirely and deferred to sprint 40, recorded in `agent-tasks/agent-tasks.md`'s
     backlog now so the idea doesn't evaporate.

**Sprint 39's final, locked scope: `--resume <path>` only** (§5 points 1–5, unchanged). No
`--save-interval` flag ships this sprint in any form.
