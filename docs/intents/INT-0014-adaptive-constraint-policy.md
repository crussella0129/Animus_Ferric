# INT-0014 — Adaptive constraint policy in the valve

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0014
- **State:** proposed
- **Work evidence:** none
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 126 direction research](../sprints/s126/sprint-research/research-report.md)

## Intent

Implement "adaptive" inside the valve, as Amalgam's
[INT-0005](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/intents/INT-0005-adaptive-action-policy.md)
defines it: a deterministic, logged function from qualified capability,
trusted task state, checked outcomes and remaining budget to the next
permitted action language and output budget. This is Animus Adaptive
Constrained Decoding (Amalgam's arm C) in Ferric's Rust core.

Adaptation uses only channels that are proven not to disturb the rendered
prompt:

- **The per-request grammar:** which offered tools may be emitted, the
  `thought` option, and argument narrowing.
- **Output and reasoning budgets.**

Ferric's existing mechanisms become the policy's vocabulary:

- The ring ceiling comes from measured capability.
- The repetition, no-progress, repeated-failure and oscillation guards narrow
  or redirect the grammar. For example, the grammar can exclude an identical
  call that was just repeated, or admit only an answer or clarification after
  repeated failures.
- Guard state is derived statelessly from the request's own message history.

Activation gate: this intent moves to `planned` only after INT-0012's static
valve has a qualification result in Amalgam's lab (T-202). If static
constraints do not advance there, this intent is re-evaluated rather than
built.

Boundaries and non-goals:

- The grammar can only narrow what Hermes offered. Model output and tool
  results can never enlarge authority or the permitted tool set.
- There is no mutation of system text, history or tool descriptions.
- The policy is stateless, and every decision carries a logged reason code.
- Ferric's abandoned recovery controller
  ([INT-0001](INT-0001-evidence-bound-autonomous-recovery.md)) is negative
  evidence, not a component to revive (Amalgam lesson L-11).

## Acceptance criteria

1. A versioned policy contract lives in the INT-0011 core and defines inputs,
   decisions, reason codes and monotonicity. Unit and conformance cases prove
   the policy is deterministic and that tool results or model text cannot
   widen the permitted set.
2. Receipts show that changing the grammar per request leaves the rendered
   prefix hash and the backend's cached-token counts unchanged from turn to
   turn on the pinned build. If they do not, adaptation is confined to session
   or compaction boundaries.
3. Paired static-versus-adaptive component runs show the adaptive arm's effect
   on correct actions, decoded tokens and loop incidence. They use the
   INT-0013 corpus plus multi-step guard scenarios. If the result is
   inconclusive, static stays the default.
4. Adaptive mode is explicit valve configuration, off by default until
   Amalgam's INT-0005 advances it.

## Rationale

Ferric's founding convictions were that the harness owns decoding and that
behavior scales to the model deterministically. Amalgam's lessons L-03 and
L-10 keep the second conviction but forbid changing the prompt or tool catalog
mid-conversation, because that breaks the cached prefix. The grammar travels
outside the rendered tokens, which makes it the candidate channel for per-turn
adaptation. Whether that channel is actually cache-neutral on the backend is
exactly what AC-2 has to demonstrate.

## Alternatives

- **Change the rings or prompt instructions every turn.** Rejected. It breaks
  prefix caching (Amalgam lesson L-10).
- **Let the model propose its own grammar.** Rejected as a source of
  authority. A model-proposed constraint could at most be an untrusted
  candidate, checked under Amalgam's INT-0006 rules.
- **Keep policy only in Hermes, in Python.** Possible if Amalgam selects the
  middleware path. The conformance corpus keeps the two equivalent.

## Consequences

- Reason codes and the policy version become durable experimental artifacts.
- A static result is a successful outcome if adaptation fails its
  marginal-benefit gate.

## Transition history

- 2026-09-26: created as `proposed`. Activation is gated on Amalgam's arm-B
  qualification result for the INT-0012 valve.
