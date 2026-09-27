# INT-0012 — The constrained valve at Hermes's provider boundary

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0012
- **State:** active
- **Work evidence:** [Sprint 126 T-12605–T-12608 build plan](../sprints/s126/sprint-plans/build-plan.md#execution-sequence); [T-12612, T-12613, T-12621 and T-12623 in the Iron backlog](../work/tasks.md#iron-backlog--size-sweep-grammar-options-verified-invariants)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 126 direction research](../sprints/s126/sprint-research/research-report.md)

## Intent

Provide an OpenAI-compatible chat-completions service, the **valve**, that
Hermes's existing custom endpoint can point at. The valve works in three
steps:

1. It accepts Hermes's ordinary request: messages plus native `tools`.
2. Using the INT-0011 core, it sends a constrained action request to a
   llama.cpp backend, with a harness-authored grammar.
3. It returns an ordinary OpenAI response: `tool_calls` for an action, or
   assistant `content` for a final answer.

Hermes therefore keeps its loop, tool validation, authorization, execution and
history unchanged. The valve is Ferric's implementation candidate for
Amalgam's arm B, the "static action constraint" that Amalgam's
[evaluation protocol](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/local-evaluation-protocol.md)
specifies but has not built.

Boundaries and non-goals:

- **Inference and policy only.** The valve never executes tools, never decides
  authorization, and holds no credentials beyond its upstream address
  (Amalgam lessons L-05 and L-06).
- **Stateless per request.** Every decision derives from the request itself,
  static configuration and a pinned calibration profile. Hermes profile and
  session isolation therefore holds by construction. A later need for state
  must key it by Hermes session identity and bring its own evidence (Amalgam
  INT-0005 AC-7).
- **Deterministic prompt bytes.** A constrained upstream request carries no
  native `tools`, so the valve renders tool descriptions into the prompt
  itself. That rendering is a fixed function of the tool catalog and the
  conversation, so backend prefix-cache reuse survives from turn to turn
  (Amalgam lessons L-10 and L-19).
- **Honest failures.** An upstream that rejects or ignores the constraint, an
  unsatisfiable schema, or a length-truncated action produces an explicit
  error or finish reason. The valve never returns a silently unconstrained
  answer or a parsed partial action (L-02).
- **Pass-through.** Requests without tools, with `tool_choice: none`, or
  carrying their own `response_format` are forwarded unmodified and labeled
  pass-through in receipts. Hermes's auxiliary calls (titles, compression,
  structured output) take this path.
- **One action per turn.** This follows Ferric's protocol. It narrows Hermes's
  parallel tool calls, and receipts record that narrowing.
- **Loopback by default.** The valve has no network-exposure features.
  Ferric's Tailscale work stays frozen.
- **Static first.** The grammar admits every tool Hermes offered in that
  request. Adaptive narrowing belongs to INT-0014.
- **Record-only measurement mode.** An explicit mode forwards every request
  byte-for-byte, streaming included, while writing the same receipts. A
  native comparison arm then runs through the same process and receipt code
  as the constrained arm, and the constraint transform is the only difference
  between them. Record-only is never the default and never labeled
  constrained.

## Acceptance criteria

1. A documented contract maps the OpenAI request and response subset Hermes
   uses to the constrained upstream request and back. The subset covers
   messages with tool calls and results, `tools`, `tool_choice`, `stream`,
   output limits, sampling, and reasoning or `chat_template_kwargs` fields.
   The contract states how a final answer, a clarification, a truncated output
   and an invalid output are represented. An explicit table says which unknown
   fields are forwarded and which are rejected; none is silently dropped.
2. With `stream: true` the valve emits OpenAI-compatible SSE. It sends the
   tool-name delta as soon as the scanner commits to a tool, then the
   arguments, then the finish reason. It keeps the stream visibly alive while
   the model decodes, so a consumer's stale-stream detector sees progress.
   Non-streaming mode returns the same final content.
3. When the client disconnects or cancels, the valve closes the upstream
   request promptly and the backend slot is released. Tests prove no upstream
   request is left running (Amalgam lesson L-13).
4. Each request writes one structured receipt with these fields:
   - the correlation id Hermes supplies, when present;
   - mode (constrained or pass-through);
   - hashes of the tool catalog, the schema and its options, and the rendered
     prefix;
   - upstream model identity;
   - upstream timings (prompt, cached and predicted tokens and rates);
   - finish reason, action validity, chosen tool and error class.

   Receipts contain no message content or secrets by default, and metrics the
   upstream does not report are marked unavailable.
5. Offline conformance tests exercise the contract against recorded or fake
   upstreams: success, truncation, a rejected `response_format`, malformed
   upstream output and client disconnect. They also use Hermes request shapes
   captured from Amalgam. The valve builds and passes in Ferric CI on Windows
   and Linux.
6. The valve refuses to start, or to serve constrained mode, unless a startup
   probe shows the upstream actually enforces a schema. It never serves
   unconstrained results under a constrained label.
7. A live readiness smoke uses a pinned llama.cpp build and the reference
   model in Amalgam's isolated lab profile. One real Hermes session completes
   at least one tool call and one final answer through the valve, with
   receipts, a stable prefix hash across turns and a successful cancellation.
   This proves readiness only. The paired arm-A/arm-B comparison and its
   advancement decision belong to Amalgam INT-0004 (T-202).
8. The grammar options from INT-0011 AC-8 (`thought` required, bounded or
   absent, and the lean-answer form) are selectable per valve instance through
   explicit configuration. The selected options appear in every receipt and in
   the schema hash. The default preserves today's behavior. A lean-answer reply
   is translated back to an ordinary assistant `content` reply exactly as a
   `task_complete` reply is.
9. Model-specific features are passed through and recorded, never
   reimplemented, and the valve never breaks them:
   - **Speculative decoding.** It is a server-side setting the valve does not
     alter. Receipts record the upstream's draft and accepted-draft token
     counts when reported, and `null` with the reason when not.
   - **Thinking and reasoning controls** (`chat_template_kwargs`, reasoning
     format and budget fields) are forwarded unchanged. Receipts record
     reasoning-token counts separately from `thought` and action tokens.
   - **Template-native tool formats** reach the upstream untouched in
     record-only and pass-through modes.

   A pass-through test with a fake upstream proves each field survives the
   constrained transform byte-for-byte.
10. History rendering has explicit, measured options. Each option is recorded
    in receipts, and each is measured by per-turn cached-token receipts on a
    hybrid (recurrent) model and on a pure-attention model:
    - echo the `thought` from `reasoning_content` (today's default);
    - omit past thoughts entirely;
    - reproduce the model's exact generated bytes where Hermes preserves
      enough to do so.

    The Sprint 126 finding is a required case. Hermes's iteration-cap summary
    request re-renders an earlier assistant turn, which broke prefix extension
    in cap-exhausted sessions. For every option, the report states whether it
    keeps the rendered prefix append-only across that request.
11. The valve's own invariants are verified for arbitrary inputs by
    property-based tests:
    - the transform is deterministic, and its projection is append-only for
      any Hermes history extended by appended turns;
    - pass-through requests are forwarded byte-for-byte;
    - a `length`-truncated upstream completion never yields `tool_calls` or a
      parsed reply;
    - an unparsable or unoffered action never reaches the client as an
      answer;
    - every chat request writes exactly one content-free receipt.

    These run in the default test lane. They complement INT-0011 AC-9 and
    use the same recorded tooling decision.

## Rationale

Amalgam's
[architecture comparison](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/architecture-comparison.md#recommended-first-architecture)
recommends exactly this shape. A narrow Rust service accepts the stable Hermes
request and tools, derives a checked constraint, calls a qualified llama.cpp
server and returns ordinary Hermes responses. Its
[Hermes seams chapter](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/hermes-decoding-seams.md)
shows the custom endpoint is the smallest attachment point that needs no core
Hermes change. The service advances only for a demonstrated protocol need, and
arm B has one: Hermes speaks native tool calls, while harness-owned decoding
needs a single constrained action object translated back into those calls.
Statelessness satisfies Amalgam's profile-isolation requirement without extra
machinery, because each request already carries the whole conversation.

## Alternatives

- **Hermes `llm_request` middleware plus a response adapter, in Python.**
  Viable. It would put the grammar logic in Python inside Hermes's process and
  would not use Ferric's tested core. Amalgam's gates may still select it if
  the service boundary proves too costly, and INT-0011's conformance corpus
  makes such a port checkable.
- **A provider plugin with its own client.** Kept for later. A custom client
  inherits Hermes's streaming, cancellation, interruption and auxiliary-call
  obligations.
- **llama.cpp's native tool grammar with a forcing `tool_choice`, treated as
  the static constraint.** That grammar is authored by the backend from the
  chat template, not by the harness. It is a measured comparison arm in
  INT-0013, not an implementation of this intent.
- **In-process llama.cpp bindings.** Rejected for the first boundary
  (Amalgam lesson L-13; Ferric ADR-027).
- **Reusing Ferric's `ferric server` and Tailscale machinery.** Rejected. That
  code is frozen, and network exposure is not needed.

## Consequences

- Amalgam's lab gains one more long-running process to own and tear down.
- Arm B's prompt bytes differ from arm A's native template rendering because
  the valve renders tool descriptions itself. Amalgam must control for prompt
  size in its comparison, and receipts make the difference measurable.
- One action per turn may cost extra turns on tasks where Hermes would batch
  calls.
- The Sprint 126 pilot showed the valve's lean prompt (about 900 fewer tokens
  than native template rendering on the 27B) is more than offset by decoding
  the forced `thought` and JSON reply. That is why grammar options and history
  rendering became acceptance criteria (AC-8, AC-10), measured per model size
  under INT-0013.
- The valve's passthrough of model-specific features makes it a reporting
  point for them. Receipts, not assumptions, say whether speculation or
  reasoning was active.

## Transition history

- 2026-09-26: created as `proposed` from the owner's direction to bring
  Ferric's constrained decoding to Hermes through Amalgam.
- 2026-09-26: added the record-only measurement mode to the boundaries so a native arm shares the valve's code path. Then moved from `proposed` to `planned` after the owner approved the Sprint 126 plan, with T-12605 to T-12608 covering AC-1 to AC-4, AC-6 and AC-7, and model-free conformance (AC-5).
- 2026-09-26: moved from `planned` to `active` when Sprint 126 Build began T-12605 (valve request transform), after the T-12602 extraction gate passed.
- 2026-09-27: revised at the owner's direction after the Sprint 126 pilot. Added AC-8 (per-instance grammar options, recorded in receipts and the schema hash), AC-9 (pass-through and recording of speculative decoding, thinking controls and template-native formats, never reimplemented), AC-10 (measured history-rendering options, including Hermes's iteration-cap re-render found in Sprint 126) and AC-11 (valve invariants verified by property-based tests). Earlier criteria are unchanged. Backlog T-12612, T-12613, T-12621 and T-12623 carry the work. State remains `active`.
