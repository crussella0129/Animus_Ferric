# INT-0008 — Unified local-model workflow

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0008
- **State:** proposed
- **Work evidence:** none
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** none

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
bounded execution, ownership-aware teardown, concurrency exclusion, truthful
failure classification, immutable evidence, and operator authorization. It
does not authorize silently deleting models or evidence, hiding a failed
check, or replacing reproducible controls with an opaque best-effort shortcut.

The public workflow must also have a cross-platform contract. Operators should
not have to translate a PowerShell runbook into a Unix shell runbook, or learn
platform-specific process and path details, to perform the same supported
operation.

## Acceptance criteria

1. One documented, installed entry point exposes the supported local workflow
   with a compact set of discoverable operations for run, status, resume,
   explain/dry-run, evidence inspection, and cleanup. Setup, artifact
   validation, and hardware calibration are composed behind that surface
   rather than published as a manual command sequence.
2. On a clean supported host, one high-level run command plus only unavoidable
   authorization prompts can reach the first managed application run. After an
   interruption or recoverable failure, one resume command continues from the
   last independently valid checkpoint without repeating completed work.
3. Repeating any non-destructive operation converges on the same valid state.
   Existing matching models, runtimes, calibration results, and application
   evidence are reused; partial or stale artifacts are detected and handled
   explicitly; concurrent invocations cannot corrupt or duplicate a run.
4. Status reports the selected model and runtime identities, current phase,
   last verified checkpoint, active ownership/lock state, retained evidence,
   failure classification, and the next safe command. It distinguishes
   incomplete, resumable, blocked, failed, and complete states and offers a
   stable machine-readable form.
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
   platform-specific scripts. Platform adapters cover paths, locking, process
   lifecycle, GPU/runtime discovery, and signal handling, with parity tests and
   documented capability differences where exact parity is impossible.
9. End-to-end usability tests cover clean setup, already-prepared rerun,
   interrupted resume, stale-state recovery, dry-run non-mutation, evidence
   discovery, concurrent invocation, and cleanup. The tests prove the compact
   surface drives the same validated lower-level controls rather than bypassing
   them.

## Rationale

Sprint 114 exposed a large gap between having rigorous local-model controls and
having an operable product: exercising one model safely required a long,
order-sensitive PowerShell runbook. That is useful as engineering evidence but
too costly and error-prone as the normal human interface. The product should
carry the phase ordering and state interpretation so the operator can express
the goal, inspect what will happen, and recover safely when reality interrupts
the happy path.

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

## Transition history

- 2026-08-27: created as `proposed` from operator feedback that Sprint 114's
  safe local-model test required an unreasonable number of manual PowerShell
  commands.
