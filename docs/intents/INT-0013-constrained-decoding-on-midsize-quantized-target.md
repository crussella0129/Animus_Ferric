# INT-0013 — Constrained decoding on the mid-size quantized target

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0013
- **State:** proposed
- **Work evidence:** none
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 126 direction research](../sprints/s126/sprint-research/research-report.md)

## Intent

Establish, at the action level and under controlled conditions, whether
harness-owned decoding makes the owner's reference class faster or more
accurate than the backend's own tool calling, and which grammar options pay
for themselves. The reference class is a ~27B dense model at about Q4
(Qwen3.8-27B UD-Q4_K_M) with partial GPU offload on an 11 GB-VRAM,
32 GiB-RAM host. The results are the component evidence that INT-0014's
adaptive policy and Amalgam's arm-B decision draw on.

The arms share the same model, backend build, context, offload, prompt corpus,
sampling and output caps:

| Arm | What it tests |
|---|---|
| **N-auto** | Native tools with the backend's default, lazily triggered template grammar |
| **N-forced** | Native tools with a forcing `tool_choice`, where the build supports it |
| **F-thought** | Ferric's unified action schema with today's required `thought` field |
| **F-bounded** | The unified schema with a length-bounded `thought` |
| **F-none** | The unified schema with no `thought` field (action only) |

A thinking axis crosses the grammar arms: backend reasoning off versus a
bounded reasoning budget, wherever the combination works. Upstream llama.cpp
does not document how a `json_schema` grammar applies to a reasoning model, so
that interaction is measured, not assumed.

Measured per action:

- structural validity, correct tool choice, and semantically correct arguments,
  judged by an independent checker against a corpus with known answers;
- decoded tokens, split into reasoning, `thought` and action;
- time to the first committed tool name, and total wall time;
- prompt and cached tokens, and grammar compile or first-token overhead;
- failures and truncations.

The headline figures are decoded tokens and wall time per **correct** action.

Boundaries and non-goals:

- **Component level only.** The corpus is single-step and short multi-step
  items with known correct actions, not long Hermes sessions. Amalgam's
  INT-0004 and INT-0007 own long sessions, context growth, compaction and
  cache stability across many turns.
- **Deadlines come from measured throughput,** following Amalgam's INT-0007
  AC-7 approach, not from fixed seconds.
- **Artifacts are pinned by hash, and nothing downloads automatically.** A new
  model class, such as a mixture-of-experts checkpoint, is an owner decision.
- **Small models stay in the fleet** (the existing 1B, ~4B and 7–8B models)
  as the floor and a regression check, not as the headline.
- **No general coding-capability claim** follows from action-level results
  (Amalgam lesson L-04).

## Acceptance criteria

1. A manifest pins the model, tokenizer and template hashes; the llama.cpp
   build and features; context, offload and KV settings; sampling; the corpus
   version; the Ferric core version; and the host class. A result missing any
   of these coordinates is marked unreproducible.
2. The action corpus covers at least four cases, and each item has an
   independent expected-action checker:
   - choosing the single correct tool from a catalog as large as Hermes's,
     not Ferric's twenty tools;
   - argument-heavy calls with paths, enums and nested objects;
   - a step where the right move is a final answer with no tool;
   - a clarification case.
3. All arms run paired and counterbalanced, cold and warm, with at least three
   pilot repetitions. All denominators, failures and exclusions are published
   (Amalgam lesson L-12). Unsupported combinations are recorded as
   unsupported, not as failures.
4. For the pinned build, the reasoning-and-grammar interaction is
   characterized with receipts: whether reasoning precedes the constrained
   action, is suppressed, or breaks the grammar.
5. A written verdict covers each grammar option: whether it reduces decoded
   tokens per correct action or raises correctness, and at what cost. A
   negative result, where the grammar costs more than it saves on this class,
   is an acceptable realized outcome.
6. With this evidence cited, the core's default schema options and the
   budget seed rows for the relevant tier are updated. Seeds based only on
   parameter count stay labeled as priors.
7. Results are published in sanitized form, pinned to a Ferric commit, so
   Amalgam's Book can cite them.

## Rationale

The owner reports that quantized models around 30B outperform 7B models even
though they are slower, and asks whether constrained decoding can make
something like Qwen3.8-27B at Q4 run faster or more accurately. Amalgam's
[direction review](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/direction-review.md)
measured this model on the target host at about 3.6 decoded tokens per second,
roughly 0.28 s per token. It concluded that decoded tokens dominate
steady-state cost and that "constrained decoding pays off in fewer tokens, not
only valid ones." Ferric's own Sprint 115 smoke of the same artifact reported
a 3.565 tokens/s median at 32,768 context with 24 of 66 layers on the GPU.

Ferric has never measured the token cost of its grammar. The required
`thought` field and its "you MUST use this" instruction arrived on 2026-07-15
as part of a grammar-hang fix. At 0.28 s per token, a 100-token thought adds
about half a minute to every action. Ferric's historical 25/25-versus-0/25
constrained-over-native result was adapter-dependent (ADR-025), so the
backend's native template grammar has to be a real arm rather than a strawman.

## Alternatives

- **Go straight to full Hermes sessions.** That is Amalgam's job. It is
  costlier and confounded by context and cache effects that this component
  evidence is meant to separate.
- **Rely on Ferric's historical results.** Rejected. They cover older, smaller
  models, and the headline comparison was adapter-dependent.
- **Measure only small models.** Rejected. The owner found them weaker for
  real work, and the design center has moved.

## Consequences

- The work needs GPU host time. Because the reference model is slow, the
  corpus must stay small and well chosen.
- Some arms may be unsupported on the pinned build, and those gaps are data.
- Updating defaults from this evidence may change Ferric's recorded
  behavior. The version bump under INT-0010 AC-4 keeps earlier results
  attributable.

## Transition history

- 2026-09-26: created as `proposed`. It supersedes INT-0007's calibration and
  measurement direction for the Iron. INT-0007's Ferric-built application
  trial and Sprint Loops compatibility probe are not carried forward.
