# Sprint 112 Implementation Plan

## Goal

Create a reproducible baseline for autonomous repository work and remove the
runtime defects that make long-horizon recovery, ambiguity, and completion
unsafe or unverifiable.

## In scope

1. Harden the benchmark runner before using it for the sprint baseline:
   isolated configuration, enforced turn budgets, concurrent output draining,
   repeated trials, exact terminal grading, executable checks, provenance, and
   per-task/per-tool statistics.
2. Add a backward-compatible recovery contract:
   `TurnCommitted`, `RecoveryCheckpoint`, and `SessionPaused`; bind recovery to
   the canonical workspace; support pause/resume/resume-of-resume; reject or
   reconcile an uncommitted tail without duplicating side effects.
3. Add structured clarification:
   `request_user_input { question, context, options }` produces a non-success
   `NeedsInput` outcome and resumable checkpoint. A supplied answer is pinned as
   a goal amendment rather than treated as a fresh unrelated task.
4. Add bounded verification:
   operator-configured `run_check` uses a named fixed argv, timeout, output cap,
   Execute permission, and no arbitrary shell. If checks are configured,
   `task_complete` is accepted only after a passing check newer than the latest
   workspace mutation.
5. Evaluate an opt-in deterministic repository brief against the frozen policy.
   It becomes a default only if the measured task matrix improves without
   increasing unsafe edits or unnecessary clarification.

## Out of scope

- Claiming general autonomous-agent reliability from the internal corpus.
- Adding model-visible `shell_exec` or arbitrary command construction.
- OS/kernel sandbox implementation.
- External benchmark score optimization or benchmark-specific prompts.
- Automatic PR merge or any branch beyond `main` and `dev`.

## Commit boundaries

1. Benchmark execution correctness and result provenance.
2. Recovery trace contract, workspace validation, and replay invariants.
3. Structured pause/clarification and resume continuation.
4. Named checks and mutation-aware completion evidence.
5. Baseline corpus/runner statistics and repository-brief experiment.
6. Documentation, ADR, task ledger, test report, and walkthrough.

## Acceptance

- Old traces still parse; completed old sessions remain non-resumable.
- A recoverable stop can resume twice, and the second run sees all committed
  results exactly once.
- A trace cannot resume in a different canonical workspace.
- Crash-window tests prove no silent post-mutation/pre-history state is accepted.
- Ambiguity pauses with a structured payload and resumes with the answer.
- A configured check cannot be forged by model text, uses fixed argv, and must
  pass after the latest mutation before completion.
- Benchmark failures are executable and fail closed; every retained row is
  attributable and parse errors are visible.
- The real server lifecycle and live-model gate pass without an offline demo
  path.

