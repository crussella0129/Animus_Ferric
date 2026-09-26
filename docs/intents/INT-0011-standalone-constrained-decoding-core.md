# INT-0011 — A standalone, versioned constrained-decoding core

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0011
- **State:** active
- **Work evidence:** [Sprint 126 T-12601–T-12604 build plan](../sprints/s126/sprint-plans/build-plan.md#execution-sequence)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 126 direction research](../sprints/s126/sprint-research/research-report.md)

## Intent

Extract the parts of Ferric that implement harness-owned decoding into a
standalone library that another harness can depend on without pulling in
Ferric's loop, tools, guard, trace, VCS, CLI, serving layer or an async
runtime. The working name is `ferric-iron`. Whether that is a new crate or a
slimmed `ferric-core` is a plan decision; the dependency boundary below is
what matters.

The core is not a decoder. It authors constraints and policy, and the backend
(llama.cpp today) enforces the token mask. Documentation and names must not
describe it as a decoder.

In scope, moved rather than rewritten:

- **Action-grammar authoring** from arbitrary tool descriptors, including an
  OpenAI-format `tools` array as Hermes sends it, together with the control
  branches (final answer or terminator, and clarification). Today this lives
  in `crates/ferric-loop/src/grammar.rs` and `terminator.rs`.
- **Action parsing** of constrained JSON and the XML fallback, with typed
  errors and the rule that a length-truncated completion is never parsed as an
  action.
- **Capability-driven protocol selection** with honest capability semantics.
  A constraint kind the backend cannot enforce is an explicit error, never a
  silently unconstrained request (Amalgam lesson L-02). This closes the gap
  Amalgam found, where the `Regex` constraint variant is accepted but never
  transmitted (`crates/ferric-provider/src/openai.rs`).
- **The deterministic capability-to-policy function**: the tier decision and
  its source, budgets, the ring ceiling, and ring filtering over a
  caller-supplied tool-to-ring map. With that map, Hermes's tools can be
  assigned rings without Ferric's builtin registry. Today this lives in
  `crates/ferric-core/src/scale.rs` and `crates/ferric-tools/src/registry.rs`.
- **The loop-guard family** (repetition, no-progress, repeated-failure,
  oscillation) as pure state machines over neutral observations of tool name,
  arguments and error outcome.
- **Constrained-JSON stream scanning**: the early committed tool name and the
  thought and summary progress signals (`crates/ferric-provider/src/stream_scan.rs`).
- **The protocol's prompt-side conventions**: how offered tools are listed to
  the model (`- name: description`, today in `crates/ferric-loop/src/run.rs`)
  and how a tool result is replayed under the constrained protocol
  (`[tool_result for NAME] output`, today in
  `crates/ferric-loop/src/projector.rs`). A second harness, such as the
  INT-0012 valve, needs these to speak the same protocol Ferric's recorded
  results used.

The XML fallback parser (`parse_action`) is not the constrained path, and it
depends on `regex`. It may stay with its consumer in `ferric-loop` until a
later increment justifies moving it without widening the core's dependency
boundary.

Boundaries:

- The move is behavior-preserving and is proven by the existing suite
  (the INT-0009 principle). Ferric's own loop and bench consume the core, so
  there is exactly one implementation and no fork.
- Default features pull in no tokio, reqwest or filesystem access.
- Identical inputs produce byte-identical schemas and decisions.
- The exported policy is truthful: fields with no runtime consumer, such as
  `uses_planner`, plan budgets and `allows_subagents`, are not exported as
  live policy. This carries INT-0006's acceptance criteria 1–3 for the
  extracted contract.

## Acceptance criteria

1. The core builds and its tests pass with a dependency tree limited to
   serialization and error crates (plus shared domain types if those move with
   it). It does not depend on `ferric-loop`, `ferric-tools`, `ferric-guard`,
   `ferric-trace`, `ferric-vcs`, `ferric-cli`, tokio or reqwest. A test or CI
   check pins that boundary so a regression fails the build.
2. `ferric-loop` and `ferric-bench` use the core for schema authoring,
   parsing, protocol selection, policy and guards, and the old in-crate
   implementations are removed. The full workspace suite passes unchanged.
3. The core accepts an OpenAI-format `tools` array and produces an action
   schema from it. Schema constructs the target backend's JSON-Schema-to-grammar
   conversion cannot express are rejected with a typed error before any
   request is sent. A fixture set records which constructs (nested objects,
   enums, arrays, optional fields, unions) are supported.
4. Requesting a constraint kind the target backend cannot enforce returns an
   explicit error. No code path drops a requested constraint and sends an
   unconstrained request.
5. Every schema and policy decision has a canonical hash and stable reason
   codes. Equal inputs yield byte-identical outputs across runs and across the
   Windows and Linux CI platforms. This carries the constraint-provenance part
   of the abandoned INT-0004.
6. A language-neutral conformance corpus ships with the core: JSON fixtures
   that map input tools, profile and observations to the expected schema,
   decision or parse result. The core's tests execute it, so a non-Rust
   consumer or a future port can prove equivalence.
7. Every public field of the exported policy has a named consumer and a
   behavior test, or it is absent from the exported contract. Unavailable
   concepts such as a planner or subagents are not represented as live
   policy.
8. Grammar features that carry token cost are explicit, versioned
   schema-authoring options. The required `thought` field can be required,
   optional, length-bounded or absent. Today's behavior is the default, so
   Ferric's recorded results stay reproducible. INT-0013 measures which
   options pay for themselves.

## Rationale

Amalgam's
[Ferric lineage chapter](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/animus-ferric.md)
records that the action-schema generator lives in `ferric-loop`, so reusing it
"would also pull loop, provider, tool, guard, trace, and VCS
responsibilities." The same chapter records the over-broad capability claim for
regex constraints. Amalgam's lessons L-01 to L-03 adopt server-enforced shape,
explicit enforcement failures and deterministic capability-to-policy mapping,
and these are exactly the pieces this intent isolates. INT-0009 required that
every decomposition deliver a concrete reuse or separability win. This one has
a named consumer: the valve in INT-0012.

## Alternatives

- **Keep the grammar in `ferric-loop` and let the valve depend on it.**
  Rejected. That imports the entire harness, which is the problem Amalgam
  identified.
- **Copy the functions into the valve.** Rejected. Two implementations drift
  apart, and Ferric's recorded bench results would stop describing the shipped
  core.
- **Port to Python first.** Deferred to INT-0010's alternatives. The
  conformance corpus in AC-6 keeps that option open.
- **Add a new token-mask engine to the core.** Out of scope. Enforcement stays
  in the backend unless Amalgam's INT-0006 gate opens.

## Consequences

- The core's public API becomes a compatibility promise, versioned under
  INT-0010 AC-4.
- `ToolDescriptor` and the guard observation type move down the crate graph,
  so many import paths change in a large but mechanical diff.
- Ring numbers become data supplied by the caller instead of a property only
  of Ferric's builtin tools.

## Transition history

- 2026-09-26: created as `proposed`. It supersedes INT-0009's decomposition
  direction and INT-0006's public-policy truthfulness for the extracted
  contract, and it carries the constraint-provenance part of the abandoned
  INT-0004.
- 2026-09-26: clarified the scope before planning. The protocol's prompt-side conventions (tool listing, constrained tool-result replay) are named as in scope, and the regex-dependent XML fallback may stay in `ferric-loop` for now. Then moved from `proposed` to `planned` after the owner approved the Sprint 126 plan, with T-12601 to T-12604 covering AC-1, AC-2, AC-4 and the adapter part of AC-3. AC-5 to AC-8 remain for later increments.
- 2026-09-26: moved from `planned` to `active` when Sprint 126 Build began T-12601 (core extraction).
