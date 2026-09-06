# INT-0008 — Unified local-model workflow

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0008
- **State:** active
- **Work evidence:** [Sprint 115 T-11414 external trace-root and resume increment](../sprints/s115/sprint-plans/build-plan.md#t-11414-add-a-safe-query-only-external-trace-root-and-truthful-resume-surface); [ordered follow-up from field-report adjudication](../sprints/s115/sprint-research/external-field-report-adjudication.md#ordered-follow-up); [Sprint 116 finalized lifecycle plan](../sprints/s116/sprint-plans/build-plan.md#execution-sequence); [Sprint 117 T-11606 recovery plan](../sprints/s117/sprint-plans/build-plan.md#execution-sequence); [Sprint 118 ownership-safe Tailscale Serve plan](../sprints/s118/sprint-plans/build-plan.md#execution-sequence); [Sprint 118 post-Loop adversarial correction](../sprints/s118/post-loop-adversarial-review.md); [stable ordered local-model backlog](../work/tasks.md#post-sprint-115--ordered-local-model-work); [post-Sprint 117 lifecycle carry-forward](../work/tasks.md#post-sprint-117-lifecycle-carry-forward); [post-Sprint 118 Tailscale proof carry-forward](../work/tasks.md#post-sprint-118-tailscale-proof-carry-forward); [Sprint 119 source-owned process refactor plan](../sprints/s119/sprint-plans/build-plan.md); [Sprint 120 human-first approved plan](../sprints/s120/sprint-plans/build-plan.md); [Sprint 121 approved explicit-budget plan](../sprints/s121/sprint-plans/build-plan.md#execution-sequence)
- **Completion evidence:** none
- **Code evidence:** [T-11414 implementation record](../work/completed-tasks.md#t-11414-sprint-115); [T-11504 partial identity-safe lifecycle implementation](../work/completed-tasks.md#t-11504-sprint-116); [T-11606 accepted lifecycle remediation](../work/completed-tasks.md#t-11606-sprint-117); [T-11801 initial adapter and typed ownership record](../work/completed-tasks.md#t-11801-sprint-118); [T-11802 crash-safe launch](../work/completed-tasks.md#t-11802-sprint-118); [T-11803 status and teardown](../work/completed-tasks.md#t-11803-sprint-118); [T-11804 doctor and operator surface](../work/completed-tasks.md#t-11804-sprint-118); [T-11805 initial lifecycle evidence](../work/completed-tasks.md#t-11805-sprint-118); [Sprint 118 direct-LocalAPI correction](../work/completed-tasks.md#sprint-118-post-loop-correction); [T-11510 Sprint 118 umbrella completion](../work/completed-tasks.md#t-11510-sprint-118); [Sprint 119 shared source-process increment](../work/completed-tasks.md#t-11901-sprint-119); [Sprint 119 source test lifetimes](../work/completed-tasks.md#t-11902-sprint-119); [T-12002 configuration](../work/completed-tasks.md#t-12002-sprint-120); [T-12003 foreground preparation](../work/completed-tasks.md#t-12003-sprint-120); [T-12004 human entry point](../work/completed-tasks.md#t-12004-sprint-120); [T-12005 provider I/O](../work/completed-tasks.md#t-12005-sprint-120); [T-12006 qualification](../work/completed-tasks.md#t-12006-sprint-120)
- **Test evidence:** [T-11414 query and CLI results](../sprints/s115/sprint-tests/unit-tests.md#t-11414-query-surface); [Sprint 116 invalidated test report](../sprints/s116/sprint-tests/test-report.md); [Sprint 116 blocking critique](../sprints/s116/sprint-tests/critique.md); [Sprint 116 failure report](../sprints/s116/failure-report.md); [Sprint 117 accepted lifecycle test report](../sprints/s117/sprint-tests/test-report.md); [Sprint 117 clean test critique](../sprints/s117/sprint-tests/critique.md); [Sprint 118 ownership-safe Tailscale lifecycle test report](../sprints/s118/sprint-tests/test-report.md); [Sprint 118 Test critique](../sprints/s118/sprint-tests/critique.md); [Sprint 118 post-Loop adversarial correction](../sprints/s118/post-loop-adversarial-review.md); [Sprint 119 source-owned process Test report](../sprints/s119/sprint-tests/test-report.md); [Sprint 120 accepted prepared-host/configuration/Python Test increment](../sprints/s120/sprint-tests/test-report.md); [Sprint 121 accepted explicit-budget increment](../sprints/s121/sprint-tests/test-report.md)
- **Documentation evidence:** [Sprint 115 external field-report adjudication](../sprints/s115/sprint-research/external-field-report-adjudication.md); [Sprint 116 lifecycle and wider-gap research](../sprints/s116/sprint-research/research-report.md); [Sprint 117 acceptance-recovery research](../sprints/s117/sprint-research/research-report.md); [Sprint 118 ownership-safe Serve research](../sprints/s118/sprint-research/research-report.md); [Sprint 118 post-Loop adversarial correction](../sprints/s118/post-loop-adversarial-review.md); [current server lifecycle and Tailscale contract](../server-configuration.md#tailscale-serve-exposure); [Source-driven process contract](../process-execution.md); [Sprint 119 Loop reconciliation](../sprints/s119/loop-review.md); [Human-first command surface](../commands.md); [Prepared-host configuration and limits](../configuration.md)

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
select a compatible engine, find or add a model by stable alias, and persist
attributable defaults. A normal `cargo r` or installed `ferric` invocation must
open that front door, not fail because the human omitted an internal command.
The product owns mechanical phase ordering, readiness checks, model metadata,
and cleanup. Humans choose the objective, an ambiguous model, and meaningful
authority/resource commitments; they should not complete a technical settings
questionnaire before they can start.

Bounded readiness and resource checks must be distinguished from capability
qualification. The expensive L0-L6 benchmark is not a prerequisite to ordinary
conversation. Unmeasured capability remains explicitly unmeasured and cannot
silently promote tool authority. INT-0007 owns measurement and profile semantics;
this intent composes qualification when it is required for the requested work.
Subsequent use should require only that front door and an objective, while the
existing expert commands remain available and script-compatible through
progressive disclosure. In non-interactive no-argument use, show a short useful
welcome and exit successfully without prompts, downloads, or process launches;
malformed explicit commands still report an error.

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
   Development and acceptance runs use source-aware Cargo commands, not direct
   invocation of build artifacts or ad-hoc background executable proofs.
   Source-defined process tests own bounded cancellation-safe cleanup and prove
   their children are reaped before reporting success; manual termination of
   leftovers never converts a failed run into successful evidence.
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
12. Normal zero-argument launch opens a useful interactive front door on a
    terminal, and a non-mutating successful welcome without a terminal. A
    prepared-host first session asks at most three meaningful decisions before
    accepting an objective; repeat use does not ask for parameter count,
    quantization, family, context, protocol, ring, or calibration commands.
    Selecting an ask-only session never grants filesystem mutation, and any
    permission to work in a folder is explicit and scoped to that folder.
    Decline, EOF, cancellation, invalid configuration, absent resources, and
    ambiguous existing ownership have bounded, actionable outcomes. Errors
    state what happened and the next safe action instead of printing a runbook.

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

The owner's Sprint 120 usability feedback makes human decision cost a first-
class acceptance concern: "simplicity is genius" means carrying the mechanical
work, not merely shortening command names or hiding failed checks. Prepared-
host usability can ship as an explicitly partial increment while clean-host
acquisition, measured hardware fit, and full resumable application execution
remain visible work; a welcome screen alone does not satisfy those outcomes.

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
readiness checks. Capability benchmarking is a separate, attributable operation,
not an undisclosed wait before first conversation. Later use becomes materially
smaller and safer because aliases, profiles, and verified checkpoints are reused
idempotently. Preserving the
advanced surface increases compatibility work, but avoids forcing CI and power
users through an opaque wizard.

The Sprint 119 process refactor is a partial safety increment, not full AC-6
or platform-parity acceptance. Windows Jobs can own nested descendant Jobs;
the Unix implementation owns a cooperative process group, not a
security boundary against group escape. Linux tests involving orphaned
descendants require an actual scoped reaper, and controlled cancellation
requires a surviving source supervisor or namespace lifetime boundary.
Immediate SIGKILL of a process that owns separate nested groups cannot be
repaired by that same process's parent-watcher thread. Broader abrupt-owner
death/group-escape containment needs a separately designed durable supervisor
or kernel-backed scope before it can contribute to the eventual AC-6/AC-8
guarantee. Until then, tests must state their boundary and fail if their own
children cannot be proved reaped; this limitation does not waive that test
success condition.

## Sprint 120 progress

Sprint 120 initially accepted a prepared-host increment at `0ec5a0e`: normal source/installed
launch, no more than three meaningful setup choices, remembered model (not file
consent), read-only description, bounded owned preparation and borrowed survival,
conservative Evidence work, byte-correct cancellable provider I/O, actionable
errors and retained traces. Code, named tests, both native CI environments and a
fresh real-model/terminal conversation support the locked portions of
AC-1/3/5/6/7/9/10/11/12. This is progress, not a state transition or realization.

Readiness is intentionally separate from capability qualification: default local
CPU/4096 settings are unmeasured, not a hardware-fit promise. A saved model choice
does not persist mutation consent, revive a calibration profile or authorize
shell/ICM. T-11509 now carries the remaining acquisition/calibration/resume and
workflow-checkpoint work. T-11707 retains ordinary-host Linux authority and
T-12024 retains synchronous Git/whole-Work cancellation; macOS and broader
ownership limits remain explicit. INT-0007's application/skill qualification is
not advanced by a short Ask response. State remains active.

Checkpoint diagnosis renewed the same partial acceptance at `4f4e4f0` with a
controlled native-test-schedule caveat. Unrelated native test bodies run serially
on both CI platforms, while product race tests retain simultaneous workers and
bounded barriers. This avoids making qualification depend on competition among
unrelated fixtures without skipping tests, raising deadlines or weakening
cleanup. A narrower shared mutex was rejected because cohort membership and
nested acquisition add correctness risks; capping Tokio workers alone would not
isolate native process creation. This is a qualification-schedule decision, not
proof of the historical Windows timeout's cause or a production performance fix.
Retained stage diagnostics and T-12027 own parallel-suite robustness; T-12026
separately owns exact Windows thread-resume/enumeration admission. A recurrence
under the canonical schedule is a blocker. Code and renewed evidence are in the
[checkpoint requalification record](../work/completed-tasks.md#sprint-120-checkpoint-requalification).
T-12028 retains the ordinary-Cargo duplicate-target warning as separate UX
cleanup. No intent realization or additional model authority is inferred.

## Transition history

- 2026-09-05: revised the active intent at the owner's direction for normal
  zero-argument launch, a measurable prepared-host decision budget, explicit
  folder authority, concise errors, and separation of readiness from expensive
  capability qualification. Added AC-12 without declaring AC-2's clean-host
  acquisition/calibration/resumption complete. Sprint 120 research records the
  repository-wide review and the proposed human-first increment; state remains
  active and no acceptance result is implied.

- 2026-09-04: clarified AC-6 at the owner's direction with the source-driven
  execution and source-owned reaping contract. Sprint 119 reviews and
  consolidates the uncommitted cleanup carryover after Sprint 118's actual
  merge. Work evidence: [Sprint 119 research](../sprints/s119/sprint-research/research-report.md).
  This is an acceptance-boundary revision, not a lifecycle state transition
  or a claim of broader workflow/platform completion.
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
- 2026-08-31: Sprint 117 passed the recovered clause-level lifecycle contract
  at corrected immutable head `b679a25` with all nineteen exact EARS commands,
  336/336 package tests, 1,089 workspace passes with four intentional ignores,
  a clean adversarial critique, and six-job CI run `33351978700`. Pre-merge
  review caught and closed a missing post-publication retained-process/listener
  authority check and restored the frozen nineteen-row result ledger before
  acceptance. This accepts the affected server-lifecycle portions of AC-3,
  AC-4, AC-6, and AC-7 and adds enabling evidence toward AC-9. The broader
  compact workflow, macOS/platform parity, calibration, model aliases, and
  model-backed application trial remain active work.
- 2026-08-31: Sprint 118 restored positive `server up --tailscale` through a
  bounded direct Tailscale LocalAPI adapter, mirrored write-ahead ownership
  journals with typed stable-node identity, and proxy-first compensation and
  teardown. Apply hashes the unmodified Serve response body for its SHA-256
  ETag, submits one exact `If-Match` compare-and-swap, and never retries an
  ambiguous mutation. Cleanup removes only the owned handler and retains the
  journal when a future LocalAPI or Tailscale version cannot be interpreted
  safely. A model-free fake LocalAPI fixture proves same-connection identity
  sandwiches, journal-before-mutation ordering, exact request scope, unrelated
  state preservation, and idempotent cleanup. The extra post-Loop adversarial
  pass superseded the earlier fixed-CLI implementation without rewriting its
  commit provenance. Clause-level Test evidence accepts the affected Tailscale
  portions of AC-3, AC-4, AC-6, and AC-7 and adds enabling evidence toward
  AC-9. This closes the ordered T-11510 umbrella while retaining its individual
  T-11801 through T-11805 implementation history. Live-tailnet behavior,
  identity/ETag atomicity, T-11806's local-path-resolution fault seam, native
  transport and macOS/platform parity, AC-8, AC-9, and the broader compact
  model-backed workflow remain active; the intent is not realized.
