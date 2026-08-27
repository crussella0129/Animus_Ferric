# Sprint 40 Research — context-budget compaction

## Decisions Reviewed
- **ADR-002** (JSONL trajectory is the source of truth): compaction MUST be traced additively —
  a new event type, old traces unaffected. This is the binding constraint on the whole design.
- **ADR-006** (the scale function is pure/deterministic): `RunPolicy.prompt_budget_tokens` (70% of
  `ModelProfile.ctx`, tier-capped — `crates/ferric-core/src/scale.rs:186-188`) is already computed
  and traced (`Event::PolicySelected`) but **never read back or enforced anywhere in `run.rs`**.
  This sprint closes that gap; it does not touch `scale.rs` itself.
- **ADR-010** (constraint/tools mutual exclusivity): the compaction summarizer call is a SEPARATE
  `CompletionRequest` (own `tools: Vec::new()`, own `constraint`), so this invariant applies to it
  independently of the main loop's request.
- **ADR-015** (`ActionProtocol` + role-framing for non-native protocols): `result_message()` already
  frames tool results as `Message::user("[tool_result for X] ...")` for `ConstrainedJson`/`TextXml`
  since those protocols have no dedicated tool role. Compaction's synthetic summary message follows
  the same established convention (`Message::user("[compacted history] ...")`) rather than inventing
  a new `Role` variant.
- **ADR-037/038** (repetition/no-progress/failure guards): established precedent for an **always-on,
  no-CLI-flag mechanism** constructed unconditionally at loop start (`RepetitionGuard::new()` etc.,
  `run.rs`). Compaction is designed the same way — no `--no-compact` flag, no `RunArgs` opt-out field
  — because the trigger is threshold-gated and provably inert on every existing (short) test/session.
- **ADR-040** (Ornstein's quarantined summarizer, `ferric-research::summarize_quarantined`): the
  MECHANISM precedent (single-shot, no-tools completion producing a condensed artifact) — but
  **explicitly not reused**, per the user's own framing when this sprint was carved out of sprint 39:
  Ornstein's summarizer is shaped for **untrusted** external content (quarantine framing, harness-
  stamped `untrusted: true`, JSON-Schema-constrained data-only output). Compaction summarizes the
  agent's own **trusted** history — a different trust tier, free-text output, own system prompt, own
  crate location (`ferric-loop`, not `ferric-research`).
- **ADR-049** (session resume, sprint 39, `crates/ferric-loop/src/replay.rs`): **direct load-bearing
  dependency**, detailed in Risks below — `replay()`'s reconstruction has no concept of compaction
  today, and without extending it, a resumed session would resurrect the FULL uncompacted history,
  silently defeating the entire point of this sprint for exactly the resume+long-session case that
  motivated it.

## Sprint goal (own words)
Nothing today stops `messages` (the flat, ever-growing `Vec<Message>` `run()` resends in full every
turn — `crates/ferric-loop/src/run.rs:193-194`) from exceeding the model's real context window over
a long session; `prompt_budget_tokens` is computed once and traced but never checked again. The user
wants a real enforcement mechanism: when the model's own reported `input_tokens` (already returned
per turn in `Completion`, no new estimation heuristic needed) approaches the policy's prompt budget,
condense older turns into one synthetic "progress so far" message via a dedicated, no-tools,
single-shot summarization call — the same MECHANISM shape as Ornstein's quarantined summarizer, but
purpose-built for trusted own-history content, not a literal reuse.

## Existing Code Survey
| File | Relevance |
| --- | --- |
| `crates/ferric-core/src/scale.rs` | `RunPolicy.prompt_budget_tokens` computation (line 188) — the budget compaction measures against. Pure/deterministic (ADR-006); untouched this sprint. |
| `crates/ferric-loop/src/run.rs` | The turn loop. `messages: Vec<Message>` grows by 1 assistant + N tool-result (+ occasional nudge) messages per turn, resent in FULL every request (`request.messages = messages.clone()`, line ~194). `completion.input_tokens` (line 250, `Event::TurnEnd`) is the exact signal needed — no separate token-estimation code exists anywhere in the loop. `RunArgs` (lines 43-75) is the extension point for any new fields. Guard constructors (`RepetitionGuard::new()` etc., lines 163-165) are the "always-on mechanism, no flag" precedent. |
| `crates/ferric-core/src/message.rs` | `Message`/`Role`/`ToolCall` shapes. No `Role` variant fits a "compaction summary" naturally; `Message::user(...)` is the established fallback framing (ADR-015 precedent already in `result_message`). |
| `crates/ferric-loop/src/run.rs` (`result_message`, `no_action_nudge`, etc., lines 512-526 and T-3903's extracted helpers) | Shows the established pattern of small `pub(crate)` formatting helpers shared between `run()` and `replay()` — the same pattern a compaction summary-message formatter should follow. |
| `crates/ferric-trace/src/event.rs` | `Event` enum — where a new `HistoryCompacted` variant must land, additively (`#[serde(default...)]` on any new fields elsewhere, matching every sprint-39 precedent). |
| `crates/ferric-loop/src/replay.rs` (sprint 39, `PendingTurn`, `replay()`) | **The direct dependency.** Reconstructs `messages` purely from per-turn trace events (`TurnStart`/`TurnEnd`/`ToolCall`/`ToolResult`) with NO concept of a mid-session history rewrite. Must be extended to recognize a new event and fold/discard turns accordingly — detailed under Risks. |
| `crates/ferric-research/src/lib.rs` (`summarize_quarantined`, `digest_schema`) | The Ornstein mechanism precedent: single-shot, `tools: Vec::new()`, own system prompt, own `CompletionRequest`. Confirmed structurally NOT reusable as-is (wrong trust framing, wrong output schema, wrong crate) — a new, smaller, purpose-built function is the right call, not a parameterization of this one. |
| `crates/ferric-cli/src/trace_cmd.rs` | `render()`'s per-event-type match — where a new `HistoryCompacted` case must be added for `ferric trace cat` legibility. |
| `crates/ferric-provider/src/lib.rs` (`Completion`, `CompletionRequest`, `Provider` trait, `MockProvider`) | Confirms `Completion.input_tokens: Option<u32>` is already returned per turn (real backends report it; `MockProvider` test fixtures set it explicitly) — this is the exact signal the design needs, with no new plumbing. |
| `agent-tasks/agent-tasks.md` (Sprint 40 backlog entry, written during sprint 39's research) | The user's own design confirmation, recorded verbatim: model-driven summarization, triggered by `input_tokens` vs `prompt_budget_tokens`, Ornstein-shaped-but-not-reused, own dedicated sprint. |
| `decisions.md` | ADRs reviewed above. |

(11 files/table rows — within the 20-file research budget; no override needed.)

## External Sources
Compaction/context-management design in comparable agent harnesses, to ground the mechanism in
established practice rather than inventing conventions from scratch:
- [Compaction | Microsoft Learn (Agent Framework)](https://learn.microsoft.com/en-us/agent-framework/agents/conversations/compaction) —
  the most directly useful reference. Key transferable ideas: **atomic message groups** (an
  assistant-with-tool-calls message and its tool-result messages must be compacted together, never
  split — directly maps onto Ferric's per-turn `[assistant, tool_result*, nudge?]` shape); system
  (and, by direct analogy, the initial task-defining user message) are **always preserved**, never
  candidates for compaction; a `MinimumPreserved`/`keep_last_groups` floor protects the most recent
  turns verbatim; summarization is one strategy among several (truncation, sliding-window, tool-
  result-collapse), typically composed as a pipeline from gentlest to most aggressive; a **separate,
  cheaper model** is recommended for the summarization call itself where the deployment has one
  available.
- [Autonomous context compression (LangChain blog)](https://www.langchain.com/blog/autonomous-context-compression) —
  model-driven (not just threshold-driven) compaction timing (task boundaries, before consuming
  large new context); retains the most recent ~10% of context verbatim; the compression action
  itself stays visible in the retained window so the agent knows compaction happened.
- Corroborating overview material (not individually fetched, cited from search-result summaries):
  progressive multi-threshold compaction (70/80/85/90/99%) in "Adaptive Context Compaction" designs,
  and the `deepagents`-style convention of compacting at **85% of the model's context limit** —
  used below as the concrete trigger-fraction precedent, since the user's own framing ("model-
  driven... approaching prompt_budget_tokens") specified the mechanism but not an exact number.

**Key divergence from every source reviewed:** all of them assume a multi-model deployment (a
cheap/fast model dedicated to summarization, distinct from the main agent model). Ferric runs
**one local GGUF model per session** (the whole point of the edge-model architecture) — there is no
second, cheaper model to delegate to. The summarizer call must reuse the SAME `provider` already
driving the loop. This is a deliberate, honest architectural difference, not an oversight the plan
needs to route around; it just means the summarization call's own cost (an extra turn's worth of
inference) is not free, which is exactly why gating it behind a real threshold (not "always
compact") matters.

## Risks, unknowns, dependencies
1. **Replay/resume interaction (the biggest risk).** Sprint 39's `replay()` reconstructs `messages`
   solely from per-turn events. If compaction is added without also teaching `replay()` about it,
   a `--resume` of a session that was compacted-then-killed would resurrect the FULL, pre-compaction
   history — silently reintroducing the exact context-overrun problem this sprint exists to prevent,
   for precisely the long-running-session case the user's own motivating scenario describes. This
   sprint's plan MUST include extending `replay.rs`, not defer it — deferring would ship a mechanism
   that quietly stops working the moment `--resume` is involved.
2. **Where the trigger check lives in the loop.** `completion.input_tokens` for turn N is only known
   AFTER turn N's completion returns — there's no way to know a request's true size in advance
   without a new estimation heuristic (explicitly out of scope per the user's own framing). The
   cleanest design checks the LAST known `input_tokens` at the TOP of each iteration, before that
   turn's request is assembled, so a triggered compaction shrinks the very next request rather than
   lagging an extra turn.
3. **Turn-boundary bookkeeping inside a flat `Vec<Message>`.** `run()` has no existing concept of
   "where does turn N's messages start/end" — it only tracks the flat vec. Compaction needs this to
   know what's safe to fold vs. what must stay intact (mirrors the atomic-group concept from the
   Microsoft Learn source). This needs its own lightweight bookkeeping structure, detailed at the
   Plan phase; it does not require restructuring `messages` itself away from `Vec<Message>`.
4. **Summarizer failure mode.** A provider error on the summarization call should not be fatal to
   the whole session (matches the project's established "non-fatal, surfaced" convention for e.g.
   media-skip in `query.rs`) — compaction should degrade to "skip this round, continue with
   uncompacted history, note it," not abort the run.
5. **No second cheaper model** (see External Sources divergence above) — the summarizer reuses the
   main `provider`, so its own turn costs real inference time; this argues for a real (not
   maximally-eager) trigger threshold.
6. **Repeated/recursive compaction over a very long session.** A session long enough to trigger
   compaction once may trigger it again later. The design needs the SECOND compaction to fold the
   FIRST compaction's summary message together with newly-eligible turns into one new summary
   (never accumulate multiple summary messages) — this is a correctness requirement for both `run()`
   and the `replay()` extension, not just an optimization.

## Recommended approach (+ alternative considered)
**Recommended:** a new `crates/ferric-loop/src/compact.rs` module, wired into `run()`'s loop as a
fourth always-on mechanism alongside the three guards (no CLI flag, no `RunArgs` opt-out — matches
ADR-037/038's precedent, and is safe because the threshold trigger is provably inert on every
existing short test/session). Trigger: last-known `completion.input_tokens` ≥ ~85% of
`policy.prompt_budget_tokens` (deepagents-style precedent, since the user specified the mechanism
but not the exact fraction). On trigger: fold all turns from the start (excluding the fixed
`[system, user]` head and any prior compaction summary, which itself becomes foldable input to a
NEW summary) up to a preserved recent-turn floor (small constant, e.g. keep the last 2 turns
verbatim — mirrors `MinimumPreserved`/`keep_last_groups`) into ONE `Message::user("[compacted
history] ...")` message, via a single-shot, no-tools, unconstrained-free-text completion against the
SAME provider. Trace it as a new additive `Event::HistoryCompacted { through_turn, summary }`.
Extend `replay()` to recognize the latest such event and reconstruct accordingly (drop turns
`<= through_turn`, insert the summary message after the head).

**Alternative considered: a fixed sliding window (drop oldest turns entirely, no LLM summarization)
instead of summarization.** Rejected as the PRIMARY mechanism because it silently discards
information the agent may still need (a file path mentioned 10 turns ago, a decision made earlier)
— exactly the failure mode "Cure"-style summarization exists to avoid, and the user explicitly
asked for **model-driven summarization**, not truncation. Note for the Plan phase: a hard
turn-count/message-count backstop (matching `TruncationCompactionStrategy`'s "emergency" role in the
Microsoft Learn source) is still worth keeping in reserve for the pathological case where even the
summarizer's own output keeps the budget exceeded — but as a last-resort fallback behind
summarization, not a replacement for it.
