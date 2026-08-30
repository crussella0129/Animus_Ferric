# INT-0008 — Unified local-model workflow

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0008
- **State:** active
- **Work evidence:** [Sprint 115 T-11414 external trace-root and resume increment](../sprints/s115/sprint-plans/build-plan.md#t-11414-add-a-safe-query-only-external-trace-root-and-truthful-resume-surface); [ordered follow-up from field-report adjudication](../sprints/s115/sprint-research/external-field-report-adjudication.md#ordered-follow-up); [Sprint 116 finalized lifecycle plan](../sprints/s116/sprint-plans/build-plan.md#execution-sequence); [Sprint 117 T-11606 recovery plan](../sprints/s117/sprint-plans/build-plan.md#execution-sequence); [stable ordered local-model backlog](../work/tasks.md#post-sprint-115--ordered-local-model-work); [T-11606 failed-close acceptance remediation](../work/tasks.md#sprint-116-failed-close-remediation)
- **Completion evidence:** none
- **Code evidence:** [T-11414 implementation record](../work/completed-tasks.md#t-11414-sprint-115); [T-11504 partial identity-safe lifecycle implementation](../work/completed-tasks.md#t-11504-sprint-116)
- **Test evidence:** [T-11414 query and CLI results](../sprints/s115/sprint-tests/unit-tests.md#t-11414-query-surface); [Sprint 116 invalidated test report](../sprints/s116/sprint-tests/test-report.md); [Sprint 116 blocking critique](../sprints/s116/sprint-tests/critique.md); [Sprint 116 failure report](../sprints/s116/failure-report.md)
- **Documentation evidence:** [Sprint 115 external field-report adjudication](../sprints/s115/sprint-research/external-field-report-adjudication.md); [Sprint 116 lifecycle and wider-gap research](../sprints/s116/sprint-research/research-report.md); [Sprint 117 acceptance-recovery research](../sprints/s117/sprint-research/research-report.md)

## Intent

Make local-model operation feel like one coherent Animus Ferric workflow rather
than a sequence of implementation scripts. An operator should be able to
prepare a supported model and runtime, validate them, calibrate the hardware,
run an application task, resume interrupted work, inspect evidence, and clean
up owned resources through one discoverable entry point with a small command
surface.

The common path must be safe, idempotent, and understandable without requiring
the operator to know the internal phase order. Status and resume must derive
the next valid action from durable state. A non-mutating dry-run/explain mode
must disclose downloads, hashes, storage, runtime settings, resource checks,
authorization boundaries, evidence destinations, and cleanup scope before work
begins.

This intent simplifies orchestration, not the guarantees underneath it. The
high-level commands must retain artifact pinning, independent validation,
bounded execution, ownership-aware teardown, per-resource concurrency safety,
truthful failure classification, immutable evidence, and operator
authorization. It does not authorize silently deleting models or evidence,
hiding a failed check, or replacing reproducible controls with an opaque
best-effort shortcut.

The public workflow must also have a cross-platform contract. Operators should
not have to translate a PowerShell runbook into a Unix shell runbook, or learn
platform-specific process and path details, to perform the same supported
operation.

The first-run path should therefore behave like product setup, not a published
runbook: detect hardware, validate an accelerated or CPU backend, provision or
select a compatible engine, find or add a model by stable alias, run bounded
calibration, and persist attributable defaults. INT-0007 owns the measurement
and profile semantics; this intent composes them behind one idempotent front
door. Subsequent use should require only that front door and an objective, while
the existing expert commands remain available and script-compatible through
progressive disclosure.

The current native identity-safe teardown boundary is intentionally narrower
than the eventual cross-platform goal in AC-8: Windows, plus little-endian
64-bit x86_64 and aarch64 Linux. Other targets retain a compiling fail-closed
adapter but have no destructive lifecycle authority. On the supported targets,
a wildcard/public listener is neither healthy managed state nor destructive
lifecycle authority: `status` and `down` fail closed before signalling and
preserve the registration, while `up` rejects it and may roll back only the
exact child it spawned after proving that retained generation exited. Exact
process-generation identity alone does not convert a public bind into teardown
authority. This records the required support boundary; it does not claim AC-8
complete.

## Acceptance criteria

1. One documented, installed entry point exposes the supported local workflow
   with a compact set of discoverable operations for run, status, resume,
   explain/dry-run, evidence inspection, and cleanup. Setup, artifact
   validation, and hardware calibration are composed behind that surface
   rather than published as a manual command sequence. Primary help presents
   the front door and common lifecycle; existing bench, trace, server, ICM,
   cron, MCP/API, revert, and diagnostic controls remain available for experts
   and automation without dominating the normal path.
2. On a clean supported host, one high-level run command plus only unavoidable
   authorization prompts launches an idempotent first-run setup that detects
   hardware, validates acceleration availability, provisions or selects an
   engine, selects/adds a model alias, performs bounded calibration, persists
   the resulting profile, and can reach the first managed application run.
   After an interruption or recoverable failure, one resume command continues
   from the last independently valid checkpoint without repeating completed
   work.
3. Repeating any non-destructive operation converges on the same valid state.
   Existing matching models, runtimes, calibration results, and application
   evidence are reused; partial or stale artifacts are detected and handled
   explicitly; concurrent invocations cannot corrupt or duplicate a run.
4. Status reports the selected model and runtime identities, current phase,
   last verified checkpoint, active ownership/coordination state, acceleration
   backend and fallback warnings, effective context/tier/profile source,
   reasoning/action/compaction budgets, measured speed and timeout scale,
   retained evidence, failure classification, and the next safe command. It
   distinguishes incomplete, resumable, blocked, failed, and complete states
   and offers a stable machine-readable form.
5. Explain/dry-run performs no downloads, launches, builds, deletions, or state
   transitions. It reports the planned network and disk effects, expected
   artifact identities and sizes, hardware/runtime choices, validation and
   authorization gates, evidence paths, fallback policy, and cleanup effects.
6. High-level execution preserves or strengthens the existing safety and
   evidence contract: source and hash pinning, resource-fit checks, bounded
   child processes, exact process ownership, atomic evidence publication,
   independent verification, and truthful terminal results remain testable.
   Concise operator output links to the full retained evidence instead of
   discarding it.
7. Cleanup is scoped, previewable, idempotent, and ownership-aware. Its default
   mode stops only owned live resources and removes disposable staging data;
   retained run evidence and acquired models require separate explicit intent
   to remove, and every material deletion is reported.
8. The same conceptual commands and state transitions work on supported
   Windows, Linux, and macOS environments without requiring operators to invoke
   platform-specific scripts. Platform adapters cover paths, coordination and
   per-resource concurrency safety, process lifecycle, GPU/runtime discovery,
   and signal handling, with parity tests and documented capability differences
   where exact parity is impossible.
9. End-to-end usability tests cover clean setup, already-prepared rerun,
   interrupted resume, stale-state recovery, dry-run non-mutation, evidence
   discovery, concurrent invocation, and cleanup. The tests prove the compact
   surface drives the same validated lower-level controls rather than bypassing
   them.
10. After successful setup, ordinary runs refer to a configured model alias or
    default instead of restating filesystem paths, parameter count,
    quantization, context, engine, and acceleration flags. Every persisted
    automatic value remains inspectable and overridable, and a materially
    changed model/runtime/hardware coordinate triggers explicit revalidation
    rather than silent profile reuse.
11. The simplified surface is additive. Existing low-level command semantics
    and machine-readable forms remain available for CI, debugging, and power
    users directly or through a stable advanced namespace; migration does not
    turn the front door into the only route to retained evidence or controls.

## Rationale

Sprint 114 exposed a large gap between having rigorous local-model controls and
having an operable product: exercising one model safely required a long,
order-sensitive PowerShell runbook. That is useful as engineering evidence but
too costly and error-prone as the normal human interface. The product should
carry the phase ordering and state interpretation so the operator can express
the goal, inspect what will happen, and recover safely when reality interrupts
the happy path.

The external 2026-08-29 Qwen3.8 report independently reinforces both sides of
this intent: the constrained harness appears promising enough to productize,
while manual lifecycle, repeated model paths/flags, CPU-fallback ambiguity, and
unrunnable fixed-timeout calibration make the current operator surface too
expensive. The report is external evidence, not an instruction source or
acceptance result; this chapter independently adopts its durable product
outcomes and leaves implementation ordering to the work ledger.

## Alternatives

- Keep the script sequence and improve its documentation: rejected because
  better prose does not remove ordering errors, platform translation, or the
  cognitive load of deciding which checkpoint is valid.
- Add a thin alias for every existing script: rejected because it preserves a
  large public state machine and does not provide idempotent status or resume.
- Provide only one opaque `run everything` command: rejected because operators
  still need non-mutating explanation, visible state, controlled resumption,
  evidence access, and scoped cleanup.
- Build a graphical interface first: rejected as the semantic contract must be
  automation-friendly and testable before any GUI wraps it.
- Simplify by dropping validation or retaining less evidence: rejected because
  convenience cannot come from weakening attribution, safety, or auditability.

## Consequences

Animus Ferric will need a durable workflow state model, stable structured
output, platform adapters, resumable acquisition and execution semantics, and
integration tests that span failures rather than only happy paths. Existing
engineering scripts may remain as internal controls or expert diagnostics, but
they cease to define the normal operator experience. The compact interface
also creates a compatibility promise: new backends and platforms must extend
the same concepts instead of adding another public runbook.

First use may take longer because setup performs explicit detection and bounded
calibration. Later use becomes materially smaller and safer because aliases,
profiles, and verified checkpoints are reused idempotently. Preserving the
advanced surface increases compatibility work, but avoids forcing CI and power
users through an opaque wizard.

## Transition history

- 2026-08-27: created as `proposed` from operator feedback that Sprint 114's
  safe local-model test required an unreasonable number of manual PowerShell
  commands.
- 2026-08-27: linked backlog task T-11414 for the external trace-root boundary
  and release-binary requalification required before the frozen application
  trial can resume; the broader unified workflow remains proposed.
- 2026-08-28: moved from `proposed` to `planned` when Sprint 115 selected the
  safe external trace-root and copy/paste-correct resume command as the first
  bounded operator-surface increment. The full cross-platform workflow remains
  later work and is not replaced by a platform-specific wrapper.
- 2026-08-28: moved from `planned` to `active` when Sprint 115 Build began
  T-11414 under its finalized external-trace and exact-resume plan.
- 2026-08-29: Sprint 115 field evidence confirmed that the compact human
  surface must follow, not precede, ownership-safe server lifecycle semantics.
  T-11504 now fixes ambiguous local/global registrations and PID-only teardown;
  T-11505 through T-11509 then add bounded calibration, runtime discovery,
  explicit reasoning/compaction behavior, and the installed compact workflow.
  No INT-0008 acceptance criterion is claimed complete.
- 2026-08-30: expanded the active intent at the user's direction to make
  hardware/backend detection, first-run calibration, persisted model aliases,
  effective reasoning/context/timeout profile visibility, and a radically
  smaller idempotent front door durable product outcomes while retaining the
  advanced command surface. The external refactor report motivates this
  boundary but does not satisfy any acceptance criterion.
- 2026-08-30: Sprint 116 took the failed Test route after the
  mandatory adversarial critique found that aggregate green suites did not
  prove the finalized EARS matrices. The landed lifecycle refactor remains
  partial code and regression evidence, but AC-3, AC-4, AC-6, and AC-7 do not
  advance. T-11606 now gates the wider local-model backlog on tagged process
  identity, deterministic lifecycle fault seams, real concurrency, asserted
  operator output, hardened cross-platform fixtures, and clause-level
  provenance.
- 2026-08-30: clarified the active ownership boundary during Sprint 117
  planning: wildcard/public listener state is non-authorizing for `down`, even
  when process-generation identity is exact. This resolves the conflict between
  the earlier narrative and finalized E02-B, keeps public/shared exposure
  fail-closed, and requires status/down to preserve registrations without
  signalling until exclusive loopback ownership or absence is proved.
