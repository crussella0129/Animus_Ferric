# Sprint 114 Failure Report

## Outcome

Sprint 114 failed at the application-execution boundary and requires a product
change before the frozen trial can resume. This is a re-architecture failure,
not an aborted sprint and not a model-quality result. T-11410 stopped during
preflight before model inference, server launch, candidate creation, or
candidate mutation. T-11412 consequently did not start.

The evidence-adjudication head was
`e8f911bdbbce7d45eb400927ad29e3c9ad84bc36`. The blocked preflight is retained
at commit `05722204243890975d7f15c9f8d2d6b09edebbf9`.

## Affected intents and unmet acceptance criteria

The affected intent is
[INT-0007 — Hardware-calibrated autonomous development](../../intents/INT-0007-hardware-calibrated-autonomous-development.md).
It remains active and must not be marked realized.

- **AC-3 — unmet:** T-11408 froze and self-tested the MH-RS01 harness, but
  Ferric never received the application task. There is no model-authored final
  workspace, application trace lineage, continuation, or independent final
  grade.
- **AC-4 — unmet:** the pre-inference stop preserved the no-Codex-repair and
  sandbox boundaries, but the required post-invocation candidate execution and
  grading never occurred.
- **AC-6 — unmet:** model/runtime calibration and the Sprint Loops packaging
  boundary were reported without inflation, but the absent application result
  prevents the required complete closeout and attribution verdict.

INT-0007 AC-1, AC-2, AC-5, and AC-7 have task-level evidence from T-11407,
T-11409, T-11411, and T-11413 respectively. Those partial successes do not
convert the intent or sprint to success.

## Unmet locked EARS clauses

- **E10-A — not reached:** the exact one-turn Ferric invocation was never
  issued because its required model-visible `run_check` could not pass the
  frozen path policy.
- **E10-B — not reached:** no first segment ended at `max_turns`, so no linked
  27-turn resume exists.
- **E10-C — not reached:** there is no final candidate, seven-dimension grade,
  mutation reconciliation, or application trace result.
- **E12-A — unmet:** both live coordinates did not finish, so the planned joint
  application/skill archive and complete evidence manifest do not exist.
- **E12-B — unmet:** T-11409 proved its own cold teardown, but T-11412 could not
  verify or tear down a nonexistent application run and its traces.
- **E12-C — unmet:** T-11412's planned truthfulness and Book-state audit did not
  execute. This failure report prevents an inflated closeout but does not
  substitute for the locked audit.

E10-D was not activated because T-11409 selected a viable Qwen3.8 Q4
coordinate. E11-B through E11-E were correctly gated as
`not-runnable-after-packaging-failure` by E11-A, and E11-F was not activated.
These conditional outcomes are not being relabeled as executed behavioral
tests.

## Root cause

The calibrated `ferric query` binary unconditionally creates
`<workspace>/.ferric/trace` before inference. The frozen MH-RS01 grader permits
only `src/`, `tests/`, and its sealed root files; any `.ferric` directory fails
the path-policy dimension. The authorized `run_check` executes with the
candidate workspace as its exact current directory, so every in-session check
would reject the trace directory before judging the application.

Deleting `.ferric` after the run cannot repair the required fresh passing
in-session check. Weakening the grader would invalidate T-11408, while changing
Ferric inside T-11410 would replace the binary qualified by T-11409 and exceed
the locked task boundary.

## Evidence

- [Preflight result](control-artifacts/app-run/preflight/result.json) records
  `blocked_pre_inference`, the calibrated Ferric SHA-256, the absent external
  trace option, source/grader bindings, the exact path collision, and cold
  execution state. Its retained digest is
  `4a55fc99a076958e49e4331a44f52a004ba87a40ecb24be52e01a2ce371d585f`.
- [App-run evidence notes](control-artifacts/app-run/README.md) explain why the
  frozen grader and calibrated binary were not modified.
- [Sprint meta](sprint-meta.md#blockages) preserves the contemporaneous
  blockage and its re-scoped, not-fixed disposition.
- T-11409's final runtime manifest
  `60cac4761f5276d05fe9f1be296925e4d9e0e7e667706564b8478bb792167f40`
  proves that the selected Q4 coordinate and old binary were viable before
  this independent layout failure.
- T-11411's [capability report](control-artifacts/sprint-loop-run/capability-report.md)
  separately establishes the Sprint Loops packaging boundary; it is not an
  application substitute.

## Recommended next state

Close Sprint 114 as `failed`. Keep INT-0007 active and not realized while its
application work remains explicitly queued. Keep INT-0008 proposed until a
later Plan phase selects its backlog work.

The required order is:

1. [T-11414](../../work/tasks.md#book-v2-carry-forward-from-sprint-114) adds a
   query-only external trace root with the existing default unchanged,
   canonical bidirectional disjointness and reparse checks before mutation,
   explicit resume behavior, fresh/resumed trace verification, documentation,
   tests, and independent release-binary requalification.
2. T-11410 then runs the unchanged frozen MH-RS01 trial against the
   requalified binary, with no Codex candidate repair.
3. T-11412 archives the completed coordinates, verifies traces and teardown,
   and publishes the non-inflated final capability verdict.

No grader edit, ad hoc trace deletion, alternate workspace shim, unqualified
binary, or fallback model is an acceptable shortcut.
