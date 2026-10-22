# INT-0013 — Where constrained decoding pays, across local model sizes

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0013
- **State:** active
- **Work evidence:** [Sprint 126 T-12609 pilot build plan](../sprints/s126/sprint-plans/build-plan.md#execution-sequence); [T-12613 to T-12618 and T-12623 in the Iron backlog](../work/tasks.md#iron-backlog--size-sweep-grammar-options-verified-invariants)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 126 direction research](../sprints/s126/sprint-research/research-report.md); [Sprint 126 pilot report](../sprints/s126/sprint-tests/pilot/report.md)

## Intent

The Iron should make local models work better regardless of the environment
they run in, and small models most of all (owner direction, 2026-09-27). The
evidence so far says that the value of harness-owned decoding is not a single
number. It varies with model capability:

- **1B (legacy Ferric evidence):** native tool calling succeeded about 22% of
  the time on single calls, while the constrained grammar reached about 100%.
- **7–8B (ADR-025):** native calling caught up to 100% once Ferric's tool-call
  adapter was repaired, so the early 25/25-versus-0/25 comparison was partly
  an adapter artifact.
- **~27B (Sprint 126 pilot):** native and constrained both completed 12/12
  tasks, and the constrained arm cost about 30% more wall time, almost all of
  it in decoding the forced `thought` field and the JSON wrapper.

This intent establishes, **for each capability profile across the local range**,
which decoding configuration is best, and at what cost. The deliverable is a
measured decision table that INT-0014's adaptive policy applies: for a given
model, whether to constrain, which grammar options to use, and which
model-specific features to enable. The measurements are action- and
short-session-level, under controlled conditions, with pre-registered
statistics.

### The axes

**1. Model capability ladder.** At least four points span roughly 1B to the
~27B reference. Where possible the points come from **one model family**, so
that size is the variable and not training recipe; for example, one family's
0.5B/1.5B/3B/7B/14B/32B GGUF ladder at the same quantization. The ladder is
joined by the Qwen3.8-27B UD-Q4_K_M reference and by the owner's existing local
models (Qwen2.5-Coder-3B and 7B). Every artifact is pinned by hash. Acquiring
any model is an owner decision.

**2. Decoding arms.** Each arm shares the model, backend build, context,
offload, prompt corpus, sampling and output caps with the others:

| Arm | What it tests |
|---|---|
| **N-auto** | Native tools with the backend's default, lazily triggered template grammar |
| **N-forced** | Native tools with a forcing `tool_choice`, where the build supports it |
| **F-thought** | Ferric's unified action schema with the required `thought` field (today's default) |
| **F-bounded** | The unified schema with a length-bounded `thought` (a `maxLength` cap) |
| **F-none** | The unified schema with no `thought` field: the action only |
| **F-lean-answer** | Where implemented (INT-0011 AC-8), a grammar in which a final reply is plain text after a fixed sentinel instead of a JSON-wrapped `task_complete` |

**3. `thought` by capability.** The `thought` field may be a genuine scratchpad
that raises correctness for small models while being dead weight for large
ones. F-thought, F-bounded and F-none are therefore compared **at every size**,
and the verdict is per size, not global.

**4. Model-specific features.** Where the model and build support them, the
following are measured as capability-conditional axes. The valve passes them
through rather than reimplementing them (INT-0012):

- speculative decoding (a separate draft model, or a model's built-in
  multi-token-prediction head), measured with the upstream's draft acceptance
  counts;
- thinking modes and reasoning budgets, crossed with the grammar arms (the
  interaction of a `json_schema` grammar with reasoning is undocumented
  upstream, so it is measured, not assumed);
- template-native tool formats, which are what N-auto and N-forced use.

A feature the model or build lacks is recorded as unsupported, not as a
failure.

**5. Environment.** "Regardless of the environment" is tested, not assumed.
Small models are also run CPU-only (`-ngl 0`) on the same host as a proxy for a
GPU-less machine. The decision table records any case where the best
configuration depends on the environment rather than on the model.

**6. Forced-token accounting.** For every constrained action, the tokens whose
value the grammar fully determines (keys, punctuation, the unique completion
of a tool name once its prefix disambiguates it) are counted. Their share of
decoded tokens gives a measured estimate of what jump-forward decoding
(appending forced tokens in one batch without sampling) would save. That
estimate is the activation evidence for, or against, a future in-process
decode engine that drives libllama with llguidance behind the valve.

### Measured per action and per session

- structural validity, correct tool choice and semantically correct arguments,
  judged by an independent checker against known answers;
- independent task completion, judged from fixture state or the final answer,
  never from the model's claim;
- decoded tokens, split into reasoning, `thought`, grammar-forced and free
  action tokens;
- time to the first committed tool name, decode time, prefill time, and total
  wall time;
- prompt, evaluated and cached tokens, and grammar compile or first-token
  overhead;
- speculative draft and acceptance counts where reported;
- failures, truncations and deadline stops.

The headline figures are **completion rate** and **decoded tokens and wall time
per independently checked completion**, per size and arm.

### Statistics (pre-registered)

Evidence is gathered in stages, so that GPU time goes to the comparisons that
matter and no claim rests on a small sample:

1. **Screening.** Every size × arm runs the frozen corpus with three paired,
   counterbalanced repetitions. Screening eliminates clearly dominated arms and
   estimates variance and discordance. It supports no claim on its own.
2. **Freeze.** Before confirmation, the sprint plan records the retained arms,
   the minimum effects that matter (15 percentage points of completion, and 20%
   in decoded tokens per checked completion), the repetition count computed
   from screening variance for 80% power at α = 0.05, and a held-out task split
   that nobody inspects before the freeze.
3. **Confirmation.** The retained arms run the frozen corpus plus the held-out
   tasks at the pre-registered repetition count.

The analysis is fixed in advance:

- **Completion:** per-cell rates with Wilson 95% intervals, and exact McNemar
  tests on paired outcomes for each arm pair within a size.
- **Cost:** paired bootstrap 95% confidence intervals (10,000 resamples) for
  the ratio of mean decoded tokens and of mean wall time per checked
  completion.
- **Multiple comparisons:** Holm correction across the pre-declared family of
  comparisons.
- **Discipline:** no optional stopping. Every denominator is published.
  Exclusions are allowed only for pre-declared infrastructure classes, with
  their reason (the Sprint 126 runner defect is the model for this), and
  excluded sessions stay in the ledger.

### Boundaries and non-goals

- **Short sessions only.** Items are single-step and short multi-step tasks
  with known correct outcomes, run through real Hermes via the valve. Amalgam's
  INT-0004 and INT-0007 own long sessions, context growth, compaction and
  cache stability across many turns.
- **Deadlines come from measured throughput,** following Amalgam's INT-0007
  AC-7 approach, never fixed seconds.
- **GGUF only, pinned by hash, and nothing downloads automatically.** Every new
  model or model class is an owner decision.
- **No general coding-capability claim** follows from action-level results
  (Amalgam lesson L-04).
- **Optimality is measured, not proved.** Formal methods are applied to the
  safety invariants of the core and the policy (INT-0011 AC-9, INT-0014),
  where proof is meaningful, not to empirical model behavior.

## Acceptance criteria

1. A manifest pins, for every run:
   - the model, tokenizer and chat-template hashes;
   - the llama.cpp build and its features;
   - context, offload, KV and speculative-decoding settings;
   - sampling, the corpus version and held-out split, the Ferric core version,
     and the host class and environment.

   A result missing any of these coordinates is marked unreproducible.
2. The action corpus covers at least these cases, and every item has an
   independent checker:
   - choosing the single correct tool from a catalog as large as Hermes's,
     not Ferric's twenty tools;
   - argument-heavy calls with paths, enums and nested objects;
   - a step where the right move is a final answer with no tool;
   - a clarification case;
   - harder multi-step tasks on which small models' native tool calling
     demonstrably fails, so the corpus is not at ceiling.
3. All arms run paired and counterbalanced, cold and warm, under the staged
   design in the Intent section. All denominators, failures and exclusions are
   published (Amalgam lesson L-12). Unsupported combinations are recorded as
   unsupported, not as failures.
4. For each pinned build, the reasoning-and-grammar interaction is
   characterized with receipts: whether reasoning precedes the constrained
   action, is suppressed, or breaks the grammar.
5. A written verdict covers each grammar option **at each size**: whether it
   raises completion or reduces decoded tokens and wall time per checked
   completion, and at what cost. A negative result, where the grammar costs
   more than it saves at some size, is an acceptable outcome.
6. With this evidence cited, the core's default schema options and the budget
   seed rows are updated per capability profile. Seeds based only on parameter
   count stay labeled as priors.
7. Results are published in sanitized form, pinned to a Ferric commit, so
   Amalgam's Book can cite them.
8. The size sweep covers at least four capability points from roughly 1B to
   the ~27B reference, preferring one family's ladder. Each point runs every
   arm, or records why it could not.
9. The statistics plan in the Intent section is written into the sprint plan
   before confirmation begins, including the minimum effects, the computed
   repetition count and the held-out split. The report states intervals and
   corrected p-values, not bare rates.
10. F-thought, F-bounded and F-none are compared at every size, and the report
    states per size whether the `thought` field raises correctness and what it
    costs in decoded tokens and wall time.
11. Speculative decoding and thinking modes are measured wherever the model and
    build support them, with draft acceptance and reasoning-token counts, and
    their interaction with the grammar arms is reported. Unsupported cases are
    recorded as such.
12. For every constrained action, grammar-forced tokens are counted, and the
    report states their share of decoded tokens and the estimated jump-forward
    saving. It then states plainly whether that estimate justifies an
    in-process decode-engine intent.
13. Small models are also measured CPU-only on the same host, and the report
    names any configuration choice that depends on the environment.
14. A versioned decision table maps capability profile (and, where it
    matters, environment) to protocol, grammar options and model-specific
    settings. Each row cites its evidence, and INT-0014's policy consumes it.

## Rationale

The owner's goal is a tool that makes local models, and small models most of
all, work better wherever they run. The owner's recollection that the grammar
improved accuracy is consistent with the evidence at the small end (the 1B
result held up), while the 7–8B comparison turned out to be adapter-dependent
and the 27B needs no help at all on an easy corpus. The Sprint 126 pilot
measured only the top of that curve, where native calling was already
perfect, so the grammar could only show its cost. A single global verdict
would be wrong in one direction or the other. A per-capability decision table
is what the evidence supports and what an adaptive policy needs.

Amalgam's
[direction review](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/direction-review.md)
measured the 27B on the target host at about 3.6 decoded tokens per second
and concluded that decoded tokens dominate steady-state cost. Sprint 126
confirmed that the `thought` field and the JSON reply wrapper are the
constrained arm's main cost: `no_tool` decoded 3 tokens natively against 51
through the valve. Ferric had never measured that cost. The required `thought`
arrived on 2026-07-15 as part of a grammar-hang fix.

## Alternatives

- **Prove the optimal configuration formally (for example in Lean).**
  Rejected. Which configuration is fastest or most accurate depends on how
  particular models behave on particular hardware, which only measurement
  reveals. A proof would restate its assumptions about the models. Formal
  verification is used where it is meaningful: the safety invariants in
  INT-0011 AC-9 and INT-0014.
- **One large benchmark run without staging.** Rejected. At about 0.3 s per
  decoded token on the 27B, running every arm at confirmation depth wastes GPU
  time on arms that screening can eliminate.
- **Measure only the ~27B reference.** Rejected. That is where the grammar
  matters least, and the owner's goal is small models regardless of
  environment.
- **Rely on Ferric's historical results.** Rejected as a verdict. They are
  used as hypotheses, but they cover older models, few trials, and one
  adapter-dependent comparison.
- **Go straight to long Hermes sessions.** That is Amalgam's job, and it is
  confounded by context and cache effects that this evidence is meant to
  separate.

## Consequences

- The work needs substantial GPU host time, and the model ladder needs owner
  approval to acquire. Staging keeps the time proportional to what is learned.
- Some arms will be unsupported on some models or builds. Those gaps are data.
- Updating defaults from this evidence changes Ferric's recorded behavior. The
  version bump under INT-0010 AC-4 keeps earlier results attributable.
- The decision table becomes a versioned artifact that INT-0014, and through
  it Amalgam, depends on.

## Sprint 126 progress

The Sprint 126 pilot (two passes, recorded in the
[E2E results](../sprints/s126/sprint-tests/e2e-tests.md) and the
[pilot report](../sprints/s126/sprint-tests/pilot/report.md)) compared native
(through the valve's record-only mode) against F-thought on the 27B, with four
tasks and three repetitions. Across 24 included sessions:

- **Completion:** native 12/12, valve 12/12.
- **Decoded tokens per request:** native 31.9, valve 42.3.
- **Wall time per session:** native 53.1 s, valve 69.7 s.
- **Cache share:** native 58%, valve 67%.
- **Prefix extension:** 12/12 in both arms.

Twelve first-pass write sessions are excluded, with their reason, after a
runner defect (a Windows verbatim working directory) made every Hermes file
write fail. They were re-run on the fixed runner.

This touches AC-1 and part of AC-3 for one size. The other sizes, arms,
statistics and verdicts remain open, so the state stays active.

## Transition history

- 2026-09-26: created as `proposed`. It supersedes INT-0007's calibration and
  measurement direction for the Iron. INT-0007's Ferric-built application
  trial and Sprint Loops compatibility probe are not carried forward.
- 2026-09-26: moved from `proposed` to `planned` after the owner approved a Sprint 126 pilot (T-12609). It covers two arms (native through record-only, and today's F-thought through the valve) on a four-task corpus with three repetitions, touching AC-1 and part of AC-3. The other arms, the reasoning axis (AC-4) and the verdict (AC-5 to AC-7) remain open.
- 2026-09-26: moved from `planned` to `active` when Sprint 126 Build began the T-12609 native-vs-valve pilot on the 27B.
- 2026-09-27: revised at the owner's direction after the Sprint 126 pilot. The desired outcome widens from "the ~27B reference" to "where constrained decoding pays across local model sizes, regardless of environment". Added the capability ladder, the F-lean-answer arm, `thought` by capability, model-specific features, the CPU-only environment axis, forced-token accounting, the pre-registered staged statistics plan, and a decision table for INT-0014. AC-8 to AC-14 were added, and AC-1, AC-2, AC-3 and AC-5 were extended; no earlier criterion was weakened. The title changed to match; the file name is kept for link stability. Backlog T-12613 to T-12618 and T-12623 carry the work. State remains `active`.
