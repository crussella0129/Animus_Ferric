# INT-0014 — Adaptive constraint policy in the valve

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0014
- **State:** proposed
- **Work evidence:** [T-12622 in the Iron backlog](../work/tasks.md#iron-backlog--size-sweep-grammar-options-verified-invariants)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 126 direction research](../sprints/s126/sprint-research/research-report.md); [Sprint 126 pilot report](../sprints/s126/sprint-tests/pilot/report.md)

## Intent

Implement "adaptive" inside the valve, as Amalgam's
[INT-0005](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/intents/INT-0005-adaptive-action-policy.md)
defines it: a deterministic, logged function from qualified capability,
trusted task state, checked outcomes and remaining budget to the next
permitted action language and output budget. This is Animus Adaptive
Constrained Decoding (Amalgam's arm C) in Ferric's Rust core.

The policy decides at two levels.

**1. Per model, at session start: which configuration this model should get.**
The Sprint 126 pilot and Ferric's earlier results show that the right answer
depends on capability. A strong model's native tool calling can already be
reliable, so constraining it only costs time. A small model's native calling
fails without the grammar. The policy therefore applies INT-0013's measured
decision table: a capability profile selects the protocol, the grammar
options and the model-specific settings.

- **Protocol:** native pass-through or the constrained action grammar.
  Native pass-through is a legitimate outcome, not a failure of the Iron.
- **Grammar options:** `thought` required, bounded or absent; the reply form.
- **Model-specific settings:** reasoning mode and budget, and whether to rely
  on the server's speculative decoding.

This decision is fixed for the session, so it cannot disturb a cached prefix.

**2. Per turn, within a constrained session: which offered actions may be
emitted next.** Adaptation here uses only channels proven not to disturb the
rendered prompt:

- **The per-request grammar:** which offered tools may be emitted, the
  `thought` option, and argument narrowing.
- **Output and reasoning budgets.**

Ferric's existing mechanisms become the per-turn vocabulary:

- The ring ceiling comes from measured capability.
- The repetition, no-progress, repeated-failure and oscillation guards narrow
  or redirect the grammar. For example, the grammar can exclude an identical
  call that was just repeated, or admit only an answer or clarification after
  repeated failures.
- Guard state is derived statelessly from the request's own message history.

### Activation gates

- **The per-model level** moves to `planned` once INT-0013 has produced its
  decision table with confirmation-stage evidence (INT-0013 AC-14).
- **The per-turn level** moves to `planned` only after INT-0012's static
  valve has a qualification result in Amalgam's lab (T-202). If static
  constraints do not advance there for any capability profile, this level is
  re-evaluated rather than built.

### Boundaries and non-goals

- The grammar can only narrow what Hermes offered. Model output and tool
  results can never enlarge authority or the permitted tool set.
- There is no mutation of system text, history or tool descriptions.
- The policy is stateless and a pure function of its declared inputs. Every
  decision carries a logged reason code and the version of the decision table
  it used.
- The capability profile comes from measurement (calibration receipts and
  INT-0013 evidence), with the model name used only as a labeled prior. An
  unknown profile falls back to a declared conservative default.
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
   INT-0013 corpus plus multi-step guard scenarios and INT-0013's
   pre-registered statistics. If the result is inconclusive, static stays the
   default.
4. Adaptive mode is explicit valve configuration, off by default until
   Amalgam's INT-0005 advances it.
5. The per-model level applies INT-0013's decision table. Given a capability
   profile, the policy selects protocol, grammar options and model-specific
   settings. An unknown profile yields the declared conservative default.
   Receipts record the chosen configuration, the profile source (measured or
   prior) and the table version. A paired run shows the per-model choice
   matches or beats both "always constrain" and "never constrain" across the
   INT-0013 capability ladder on completion and on cost per checked
   completion.
6. The policy's safety properties are **verified for all inputs**, not only
   sampled by example tests:
   - **Monotonicity:** the admitted action set is a subset of the offered
     tools plus the control branches. No input widens it.
   - **Determinism:** equal declared inputs produce equal decisions, and no
     undeclared input (clock, randomness, message text outside the declared
     fields) affects the result.
   - **Reason-code totality:** every decision carries exactly one reason code
     from the versioned set.
   - **Budget monotonicity:** chosen output and reasoning budgets never exceed
     the caps in the request or the configuration.

   Kani proves these on the actual Rust functions for bounded inputs.
   Property-based tests cover unbounded inputs in the default test lane. The
   sprint that implements the policy records whether a machine-checked Lean
   proof of monotonicity is warranted, for example via the Aeneas Rust-to-Lean
   translator or a hand model with a refinement argument. It is taken when the
   decision function outgrows what Kani can bound.
7. Adversarial fixtures, meaning tool results and model text written to look
   like policy directives, leave every decision exactly as the declared inputs
   dictate. This is tested directly, in addition to the determinism property.

## Rationale

Ferric's founding convictions were that the harness owns decoding and that
behavior scales to the model deterministically. The owner's direction of
2026-09-27 sharpens both: the tool should make local models work better
regardless of environment. The Sprint 126 pilot showed that "always constrain"
can cost a strong model about 30% in wall time for no correctness gain, while
earlier evidence showed small models need the grammar. A policy that chooses
per capability is the only one consistent with both results.

Amalgam's lessons L-03 and L-10 keep deterministic capability-to-policy
mapping but forbid changing the prompt or tool catalog mid-conversation,
because that breaks the cached prefix. The per-model decision is made at
session start for that reason. The per-turn grammar travels outside the
rendered tokens, which makes it the candidate channel for per-turn adaptation.
Whether that channel is actually cache-neutral on the backend is what AC-2
has to demonstrate.

The policy is the one component whose safety claim, that it can only narrow
and never widen, must hold for every input a hostile tool result could
produce. That makes it the right target for formal verification, unlike the
empirical question of which configuration performs best, which INT-0013
answers by measurement.

## Alternatives

- **Always constrain.** Rejected by the Sprint 126 evidence: a pure cost for a
  model whose native calling is already reliable.
- **Never constrain.** Rejected by the earlier small-model evidence, where the
  grammar made the difference between failing and succeeding on single calls.
- **Choose by model name or parameter count alone.** Rejected as the decision
  basis. Names and sizes are priors; measured capability decides, and the
  prior is labeled as such.
- **Change the rings or prompt instructions every turn.** Rejected. It breaks
  prefix caching (Amalgam lesson L-10).
- **Let the model propose its own grammar.** Rejected as a source of
  authority. A model-proposed constraint could at most be an untrusted
  candidate, checked under Amalgam's INT-0006 rules.
- **Keep policy only in Hermes, in Python.** Possible if Amalgam selects the
  middleware path. The conformance corpus keeps the two equivalent.

## Consequences

- Reason codes, the policy version and the decision-table version become
  durable experimental artifacts.
- Native pass-through for strong models becomes a normal policy outcome, and
  the valve stays useful for them as a recording point.
- Verification adds Linux-only Kani jobs and property-test tooling, under the
  allowlist decision recorded for INT-0011 AC-9.
- A static result is a successful outcome if adaptation fails its
  marginal-benefit gate.

## Transition history

- 2026-09-26: created as `proposed`. Activation is gated on Amalgam's arm-B
  qualification result for the INT-0012 valve.
- 2026-09-27: revised at the owner's direction after the Sprint 126 pilot. The policy now decides at two levels: a per-model level at session start, which applies INT-0013's decision table (including native pass-through for models that do not need the grammar), and the existing per-turn level. Added AC-5 (per-model selection from measured capability, paired against always- and never-constrain), AC-6 (monotonicity, determinism, reason-code totality and budget monotonicity verified by Kani and property tests, with a recorded Lean decision) and AC-7 (adversarial fixtures). The per-model level is gated on INT-0013's decision table; the per-turn level keeps the arm-B gate. Backlog T-12622 carries the policy contract and proofs. State remains `proposed`.
