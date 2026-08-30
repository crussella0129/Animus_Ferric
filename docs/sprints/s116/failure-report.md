# Sprint 116 Failure Report

## Disposition

Sprint 116 reached Test with valuable implemented and observed lifecycle
behavior, but it did **not** prove its finalized EARS contract. The earlier
pass report was written before the required adversarial test critique and is
invalidated. This is a re-architecture/testability failure, not a claim that
all merged behavior is broken.

## Affected intent

- [INT-0007 — Hardware-calibrated autonomous development](../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
  remains active and unchanged. Sprint 116 promoted the external report's
  calibration outcomes into AC-9 through AC-14 but did not test them.
- [INT-0008 — Unified local-model workflow](../../intents/INT-0008-unified-local-model-workflow.md)
  remains active.
- AC-3, AC-4, AC-6, and AC-7 received partial implementation evidence but are
  not advanced by a passing Sprint 116 verdict.

## Unmet acceptance evidence

- E01-A through E01-D lack the finalized combined inventory, schema, real
  concurrency, and full conditional-removal matrices. Schema validation also
  accepts an arbitrary nonempty process start token instead of the planned
  tagged OS token.
- E02-A through E02-C lack deterministic PID-remap, retained-handle, and
  spawned-child boundary fault injection.
- E03-A through E03-C lack the complete resolver/consumer matrix and asserted
  status output contract. Absent scope/origin and next-action rendering are
  not proven.
- E04-A through E04-E lack injected terminate/wait/listener-release failures,
  full cleanup outcomes, and copy/paste legacy-guidance output assertions.
- E05-A/E05-B lack short-write/durability/child-exit/compensation failure
  injection and proof that recovery paths survive every failed rollback.
- E05-D proves pre-spawn `server up --tailscale` refusal, but not the complete
  doctor plus live/absent captured-registration matrix promised by the plan.
- E05-C is strongly supported by the model-free CLI fixture on Windows and
  native WSL Linux.

The exact concern record is the
[Sprint 116 test critique](sprint-tests/critique.md).

## Root cause

The implementation was reviewed against safety behavior, but the Test verdict
was issued from aggregate suite counts rather than the finalized clause-level
traceability map. Planned fault-injection seams and named matrices were never
built. The feature-gated E2E also remained outside normal CI, so green CI could
not independently support that claim.

## Retained evidence

- Implementation commit:
  `fb05f6b17427b1e4843e703280e2f543ac5c2611`.
- Original closeout head and successful PR CI:
  `d450a755236c100fd1d9f67b2511435465a08989`, GitHub Actions run
  [33294229347](https://github.com/crussella0129/Animus_Ferric/actions/runs/33294229347).
- Owner-merged head and successful post-merge CI:
  `e6439b1eb4851d2262b6d1be973ff3098e65c3a4`, GitHub Actions run
  [33320491690](https://github.com/crussella0129/Animus_Ferric/actions/runs/33320491690).
- A post-merge local `cargo test --workspace --all-features --locked` passed on
  Windows outside the restricted process sandbox, with Rust sources matching
  `e6439b1`; this was an observed local result, not a retained immutable raw
  log. A restricted attempt failed only because nested Python launch was denied
  (`os error 5`); the exact benchmark integration rerun then passed 6/6 outside
  that sandbox.
- The three feature-gated lifecycle fixtures had passed locally on Windows and
  native WSL Linux, but no ordinary CI job ran them.

These observations remain useful regression evidence. They do not substitute
for the missing EARS matrices.

## Checkpoint protocol defect

PR #102 was opened and owner-merged before the required Test critique and
before a legal Loop close. Its sprint metadata used the unsupported exit value
`completed`, so the Book router correctly continued to treat Sprint 116 as
active. The critique, failure report, intent reconciliation, and atomic failed
close therefore remain unlanded correction work after that merge.

Project policy permits one PR per sprint, while carrying these corrections into
Sprint 117 would make the next checkpoint span two sprints. Loop therefore
stops before any second remote checkpoint or Sprint 117 initialization. The
non-bundling recovery is a one-time owner-approved `dev` to `main` correction
PR (or another boundary the owner explicitly chooses); no exception is assumed
from the earlier merge.

## Recommended next state

Route to Loop, append a closed-partial correction to T-11504's Build-phase
record, and create a bounded remediation task before report-driven calibration
or compact-command work resumes. The remediation order is:

1. enforce tagged OS start tokens and lossless origin/scope diagnostics;
2. add dependency-injected lifecycle/store fault seams and the E02, E04,
   E05-A, and E05-B negative matrices;
3. add real two-client/process concurrency coverage;
4. assert status, legacy adoption guidance, and the complete Tailscale blocked
   surface;
5. serialize/harden the feature fixture and add Windows/Linux CI jobs for it;
6. bind every EARS clause to executed names, immutable heads, exact commands,
   and an accepted final critic verdict.

Do not start the wider timeout/output-budget, backend calibration, reasoning,
or compact-front-door increments until this lifecycle foundation has a
truthful reviewed Test outcome.
