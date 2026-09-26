# INT-0010 — Ferric is the Iron of Animus Amalgam

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0010
- **State:** active
- **Work evidence:** [Sprint 126 T-12610 build plan](../sprints/s126/sprint-plans/build-plan.md#execution-sequence)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 126 direction research](../sprints/s126/sprint-research/research-report.md)

## Intent

On 2026-09-26 the owner redirected the project. They adopted Hermes Agent and
forked it as [Animus Amalgam](https://github.com/crussella0129/Animus_Amalgam),
and they find its agent and CLI experience far more intuitive than Ferric's.
Hermes has no harness-owned constrained decoding. The stated goal is to merge
"the Iron of Ferric with the Mercury of Hermes to make Amalgam."

Ferric's purpose is therefore to supply the iron. That means the harness-owned
constrained-decoding core, and the evidence about it, delivered in a form
Amalgam can consume at Hermes's provider boundary without taking on Ferric's
loop, tools or CLI. Hermes, inside Amalgam, owns the conversation, tools,
authorization, execution, memory and human surface.

Ownership is divided as follows.

| Concern | Owner |
|---|---|
| Constrained-decoding core library ([INT-0011](INT-0011-standalone-constrained-decoding-core.md)) | Ferric |
| OpenAI-compatible constrained valve service ([INT-0012](INT-0012-constrained-valve-at-hermes-boundary.md)) | Ferric |
| Component-level qualification of grammar options on the target class ([INT-0013](INT-0013-constrained-decoding-on-midsize-quantized-target.md)) | Ferric |
| Adaptive constraint policy implementation ([INT-0014](INT-0014-adaptive-constraint-policy.md)) | Ferric, gated by Amalgam's result |
| Hermes configuration, provider/plugin/middleware integration, and any change inside Hermes | Amalgam |
| System-level session qualification and the advancement decision for each arm | Amalgam ([INT-0004](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/intents/INT-0004-bounded-local-model-qualification.md), [INT-0005](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/intents/INT-0005-adaptive-action-policy.md), [INT-0007](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/intents/INT-0007-decode-budget-long-session.md)) |
| A new decoder or token-mask engine | Neither, until Amalgam's [INT-0006](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/intents/INT-0006-constraint-engine-research.md) gate opens |

The target model envelope moves up. The owner reports that quantized models
around 30B work better than 7B models, even though they are slower. The design
center becomes mid-size quantized GGUF models, roughly 20–35B at about Q4, on a
consumer GPU plus system RAM with partial offload. The Qwen3.8-27B Q4 artifact
already on disk is the reference model. Small models (1B–8B) remain supported
as the floor and the regression fleet, but they are no longer the design
center. The hypothesis tested under INT-0013 is that harness-owned decoding
can make that class faster (fewer decoded tokens, no malformed-call retries)
or more accurate, not merely usable.

Standalone Ferric moves to maintenance. This covers `ferric run`, chat, server
lifecycle and Tailscale Serve, ICM, cron, Ornstein, skills, MCP, Animus Launch
and the autonomy runner. Maintenance means these surfaces keep compiling and
keep CI green, and correctness or security fixes are accepted, but no new
features are added. Retiring or deleting any of them requires its own
owner-approved intent. `ferric-loop` and `ferric-bench` stay active, but only
as the core's first consumer and as its qualification instrument.

Non-goals:

- porting Ferric's loop or its abandoned Evidence controller into Hermes
  (Amalgam lessons L-06 and L-11);
- rewriting Hermes in Rust, or making Ferric's CLI compete with Hermes;
- claiming task success from grammatical validity (L-04).

## Acceptance criteria

1. The root `README.md`, `docs/README.md` and `docs/introduction.md` each state
   four things: Ferric's role as Amalgam's constrained-decoding donor, the
   ownership split above, the maintenance status of the standalone CLI, and
   the retargeted model envelope. No page presents the standalone CLI as the
   project's primary direction, and descriptions of shipped behavior stay
   truthful.
2. Every intent that predates this direction has a recorded disposition
   (superseded or abandoned) with its reason, and no proposed, planned or
   active intent directs new standalone-surface features.
3. The work ledger separates maintenance from Iron work. Each open task either
   links a live Iron intent or sits in a clearly labeled maintenance section
   that names the closed intent it came from. No task is deleted.
4. The core and the valve are published under a versioned identifier (a git
   tag or crate semver) with a changelog of contract changes. Amalgam's Book
   can therefore pin the exact Ferric version its evidence used, and any
   contract-breaking change bumps that version.
5. The project instructions (`CLAUDE.md` and `AGENTS.md`) tell future sessions
   the direction: Iron work first, standalone surfaces in maintenance only.
6. CI on `main` is green on both platforms, so the frozen standalone surfaces
   stay buildable and tested.

## Rationale

The owner's experience settles the product question. Hermes already has a
better human surface, a learning loop, skills, memory, a gateway and cron, and
Ferric duplicates several of them less well. What Hermes lacks entirely is
the part Ferric was founded on: the harness authors the action grammar and the
backend enforces it, so a malformed tool call cannot be produced.

Amalgam's Book reached the same boundary independently. Its
[architecture comparison](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/architecture-comparison.md)
keeps Hermes's loop and executor and recommends a narrow Rust
policy/protocol service in front of llama.cpp. Its
[Ferric lineage chapter](https://github.com/crussella0129/Animus_Amalgam/blob/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs/lineage/animus-ferric.md)
finds Ferric's value concentrated in a small, language-independent part
(deterministic capability-to-policy mapping and server-enforced action shape),
buried in a crate that drags the rest of the harness along. About 35K lines of
serving, Tailscale and process-ownership plumbing, plus the human front door,
do not serve that goal.

## Alternatives

- **Keep developing Ferric as a standalone agent.** Rejected by the owner's
  direct comparison with Hermes.
- **Move Ferric's Rust code into the Amalgam repository.** Not selected now.
  Amalgam keeps upstream syncs cheap by adding files only under `docs/`
  (Amalgam INT-0001), and a separate Rust repository with versioned releases
  keeps both trees clean. This can be revisited if Amalgam adopts a Rust
  component tree.
- **Port the core to Python inside Hermes.** Kept as an alternative. The core
  is small enough to port, and INT-0011's conformance corpus would let a port
  prove equivalence. It is not first because a port would fork the
  implementation away from Ferric's tests and bench, and Amalgam's advancement
  gates, not preference, should decide whether a service boundary costs too
  much.
- **Delete the standalone surfaces now.** Rejected. Deletion is irreversible
  scope, and those surfaces still carry tests and evidence. Retirement gets
  its own intent once Amalgam covers them.
- **Archive Ferric and start again in Amalgam.** Rejected. Ferric's bench,
  calibration history and test corpus are the evidence base for the core.

## Consequences

- Ferric sprints become smaller and cross-repository. Some evidence (live
  Hermes sessions) is produced in Amalgam's lab and cited from Amalgam's Book.
- Frozen code still costs CI time and dependency updates. That cost is
  accepted until a retirement intent is approved.
- The README's framing "built for small GGUF models" changes. Historical
  sprint records keep their original framing because they are provenance.

## Transition history

- 2026-09-26: created as `proposed` from the owner's direction to merge Ferric
  (the Iron) with Hermes Agent (the Mercury) as Animus Amalgam. Sprint 126
  research records the survey of both Books and both codebases.
- 2026-09-26: moved from `proposed` to `planned` after the owner approved the Sprint 126 plan. T-12610 covers AC-1, AC-3 and AC-5 (charter documentation, ledger triage, project instructions), and the sprint PR restores AC-6's green `main`. AC-4 versioning is not scheduled.
- 2026-09-26: moved from `planned` to `active` when Sprint 126 Build began T-12610 (charter documentation and ledger triage).
