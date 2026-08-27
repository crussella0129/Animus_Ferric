# Sprint 42 Research — raw chat mode

## Decisions Reviewed
- **ADR-011** (no chat catch-all — REVISED 2026-06-29): the original decision said "No REPL/chat
  mode will exist." The revision (recorded in memory + ADR-046's closing line, not yet its own ADR
  number) approved building chat mode as the second half of the split, **explicitly requiring its
  own dedicated ADR on the chat security boundary** — this sprint writes that ADR.
- **ADR-005** (security is hardcoded/harness-owned; the LLM is never consulted on a security
  decision): binds chat mode identically — whatever a chat turn does to the workspace still passes
  through `ferric-guard`.
- **ADR-010** (constraint/tools mutually exclusive per request) + **ADR-015** (`ActionProtocol`):
  the `run()` loop always frames actions through a constrained protocol — there is NO unconstrained
  completion path in the harness today. Chat mode's core design question is whether it reuses that
  path or opens a new one.
- **ADR-046** (`ferric mcp` — launch-time-fixed containment, one exposed action, "a new entrypoint,
  not a new decoding path or a new privilege"): the direct precedent. Its closing line deferred
  chat mode to "its own future sprint + own dedicated ADR." This is that sprint.
- **Sprint-25 `--chat`** (dropped): a *capability fallback* for models too weak to agent, removed
  once Gemma 4 E4B proved it unnecessary — a DIFFERENT thing from this IDE-integration-driven chat
  mode (memory `animus-suite-direction`). Not to be confused or resurrected.

## Sprint goal (own words)
Build the raw chat mode the ADR-011 revision approved — a genuinely conversational surface (the
literal reversal of ADR-011's "no REPL/chat mode" clause), motivated by Animus IDE wanting to send
one-off natural-language change requests conversationally. The revision flagged this as touching the
"harness always owns decoding" thesis, so **the security boundary is the central decision** and is
the user's call.

## Existing Code Survey
| File | Relevance |
| --- | --- |
| `crates/ferric-cli/src/main.rs` | The `Command` enum — a new `Chat` variant slots in exactly as `Mcp` did (ADR-046). Module doc already names chat mode as the unbuilt ADR-011-revision half. |
| `crates/ferric-cli/src/query.rs` | `run_with_provider()` (line 718) is the shared "drive the loop once" function both `query` and `mcp` call — it builds `RunArgs` and calls `ferric_loop::run()`. A chat REPL would call it once per user turn. `run_query`'s config/provider/workspace setup (`build_run_config`, backend resolution) is the reusable scaffolding. |
| `crates/ferric-cli/src/mcp.rs` | ADR-046's launch-time-fixed containment pattern (workspace/backend/model pinned at launch, one exposed action, errors-never-crash) — the closest template for chat mode's own containment. `ferric mcp` already runs "a full constrained query per message"; chat mode is that with a human REPL UX + conversation memory. |
| `crates/ferric-loop/src/run.rs` | `run()` ALWAYS routes actions through `ActionProtocol` (constrained JSON / native tools / TextXml) + the guards + `ferric-guard`. **There is no unconstrained completion path anywhere.** Sprint 39's `resume`/`ReplayedState` already gives a way to carry prior turn history into a new `run()` — the mechanism a multi-turn chat would build on. |
| `crates/ferric-loop/src/replay.rs` | `ReplayedState { messages, turns, last_text, protocol, source_session }` — the in-memory conversation-carrying type. A chat REPL keeps this across turns (in memory, no kill/replay needed) or re-seeds each turn. |
| `crates/ferric-guard/src/checker.rs` | The workspace/deny-list boundary (ADR-005) — binds every filesystem action a chat turn takes, unchanged. |

(6 files — within the 20-file research budget.)

## External Sources
None fetched — this is an internal architecture/security-boundary decision grounded in the
project's own ADRs and code, not an external-practice question (unlike sprint 41's containerization
research). The relevant "prior art" is the harness's own constrained-decoding thesis and ADR-046's
`ferric mcp` precedent, both surveyed above.

## Risks, unknowns, dependencies
1. **The security boundary is a genuine product decision, not a technical default** — three
   materially different shapes exist (below), each with a different safety posture and a different
   fit for the Animus-IDE motivation. Picking wrong either over-restricts (chat can't do the
   IDE-requested changes) or expands the attack surface (an unconstrained path that can act). This
   is the AskUserQuestion this sprint turns on.
2. **REPL/stdin mechanics on the CLI** — a chat mode needs a read-eval-print loop reading stdin
   turn-by-turn; the existing subcommands are all one-shot. Modest new plumbing (a plain stdin line
   reader), but it's the project's FIRST interactive surface — worth a deliberate minimal design
   (plain line-reading first, no fancy TUI).
3. **Conversation state across turns** — the `run()` loop is one-shot per call. A chat turn N+1
   must carry turns 0..N's history. Sprint 39's `ReplayedState` is exactly this shape and can be
   threaded turn-to-turn in memory (no trace kill/replay needed) — a clean reuse, not new
   machinery.
4. **Trace/audit** — every chat turn should still be JSONL-traced (ADR-002). One trace file per
   chat SESSION (many turns) vs. one per turn is a small design choice.

## Recommended approach (+ alternative considered)
The security boundary has three shapes; **this needs the user's decision before planning** (posed
via AskUserQuestion):
- **A — Conversational agent (constrained, recommended default):** each chat turn drives a FULL
  constrained agentic loop (reusing `run_with_provider`), with conversation history carried across
  turns. The model can act on the workspace, but ONLY via the guarded tool path — no new decoding
  path, no new privilege (exactly ADR-046's "new entrypoint, not new privilege" applied to a REPL).
  Best fit for the Animus-IDE motivation (natural-language change requests that actually change
  things). This is the safest option that still *does* things.
- **B — Talk-only chat (unconstrained completion, no tools):** a genuinely raw conversation where
  the model just talks — NO tools, NO workspace actions, NO constraint. Safe precisely because it
  can't act (reverses ADR-011's letter but not its security spirit). Opens the harness's FIRST
  unconstrained-completion path, but with zero action channel. Good for "explain this", "what
  should I do" — advice, not changes. Does NOT satisfy the IDE-change-request motivation alone.
- **C — Hybrid:** conversational/advisory by default (B), with explicit escalation of a turn to the
  constrained agentic loop (A) when the user asks for an action. Most flexible, most surface area to
  design and secure.

**Recommended: A** (conversational constrained agent) — it satisfies the IDE motivation, adds no
new decoding path or privilege, and is a clean reuse of `run_with_provider` + `ReplayedState`. **But
the security boundary is explicitly the user's call** (the ADR-011 revision named it as such), so
this is posed as a decision, not assumed. The alternative (B, talk-only) is genuinely different in
intent — worth confirming which the user wants before committing the plan.

## Scope Decided (user, 2026-07-09, after research)
The user chose **C — Hybrid (talk + escalate)**: advisory/conversational by default (an unconstrained
talk completion), with explicit escalation of a turn into the constrained agentic loop when the user
asks for an action. Binding design constraints this locks in for the Plan Phase:
- **Talk mode (default) is the harness's FIRST unconstrained-completion path** — it must be
  structurally incapable of acting: a single completion with **no tools and no constraint**, whose
  output is treated as **text only** (never parsed for tool calls, never dispatched, never touches
  the registry or `ferric-guard` because it never produces an action). Safety is structural (the
  talk path simply doesn't call dispatch), not a prompt instruction.
- **Escalation is USER-initiated, never model-initiated** (ADR-005 — the LLM is never consulted on a
  security decision; a model deciding on its own to act on the workspace would violate that). The
  user explicitly promotes a specific request to the agentic loop (e.g. a `/do <request>` REPL
  command); the model can never self-escalate from talk mode into acting.
- **Escalated turns reuse the EXISTING constrained path unchanged** — `run_with_provider` +
  `ferric-guard` + the guards + JSONL tracing, exactly as `ferric query`/`ferric mcp`. No new
  decoding path or privilege on the action side; the only genuinely new surface is the talk
  completion, which has no action channel.
- **Conversation history carries across turns** (both talk and escalated) via sprint 39's
  `ReplayedState`-shaped in-memory `Vec<Message>`, threaded turn-to-turn.
- **A dedicated ADR (ADR-052)** documents this boundary explicitly (what talk mode can/can't do,
  the user-initiated-escalation rule, why the talk path is structurally safe) — the ADR-011
  revision required exactly this.
Deferred: a fancy TUI (plain stdin line-reading first); talk-mode streaming polish; wiring chat into
the Animus IDE (a separate organ). Confirmed via `AskUserQuestion`.
