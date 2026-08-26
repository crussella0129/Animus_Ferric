# INT-0001 — Evidence-bound autonomous recovery

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0001
- **State:** abandoned
- **Work evidence:** [Sprint 113 build plan](../sprints/s113/sprint-plans/build-plan.md)
- **Completion evidence:** [frozen candidate falsification](../sprints/s113/sprint-tests/development-screen.md), [planner rejection](../sprints/s113/planner-decision.md), [Sprint 113 build closeout review](../sprints/s113/sprint-review/review-report.md)
- **Code evidence:** [evidence controller](../../crates/ferric-loop/src/controller.rs), [controlled dispatch](../../crates/ferric-loop/src/controlled_dispatch.rs), [controlled tools](../../crates/ferric-tools/src/control.rs), [paired autonomy runner](../../crates/ferric-cli/src/autonomy_cmd.rs)
- **Test evidence:** [dispatch tests](../../crates/ferric-loop/tests/evidence_dispatch_tests.rs), [controlled mutation tests](../../crates/ferric-tools/tests/controlled_mutations.rs), [autonomy result tests](../../crates/ferric-bench/src/autonomy_results.rs), [frozen development screen](../sprints/s113/sprint-tests/development-screen.md), [confirmation skip audit](../sprints/s113/sprint-tests/confirmation-skip.md), [held-task and teardown audit](../sprints/s113/sprint-tests/held-and-teardown.md)
- **Documentation evidence:** [Sprint 113 research](../sprints/s113/sprint-research/research-report.md), [approved engineering contract](../sprints/s113/engineering-contract.md), [planner decision](../sprints/s113/planner-decision.md), [build closeout review](../sprints/s113/sprint-review/review-report.md)

## Intent

Make Ferric's evidence harness materially improve a pinned small model's
multi-turn repository work by binding content-sensitive actions to observed
workspace bytes and verification feedback. The harness must preserve causal
truth across tracing, replay, recovery, compaction, product surfaces, and
benchmark attribution.

The intervention is general controller behavior. It must not add task-specific
instructions, widen the model to arbitrary shell execution, change the pinned
model or graders, or label an unimplemented planner path as available.

## Acceptance criteria

1. Evidence sessions record versioned, typed observations, controller blocks,
   measured workspace effects, named-check attempts, controller checkpoints,
   and recovery packets; legacy traces remain readable as legacy policy.
2. An existing content target cannot be mutated without fresh, complete,
   prior-turn evidence; stale/no-effect/unsafe syntax transitions are blocked
   before publication, while supported real effects advance one epoch exactly
   once. Syntax validation in every harness policy must parse model-authored
   bytes without implicitly executing an interpreter or importing workspace
   code.
3. A failed named check creates a repair-inspection barrier, and the same check
   at the same mutation epoch is refused before another process is spawned.
4. Resume, resume-of-resume, clarification, crash-prefix recovery, and
   compaction preserve controller truth and render a byte-stable recovery
   packet independently of model-authored summaries.
5. Query and supported product surfaces select or inherit harness policy
   consistently; explicit resume mismatch fails before mutation; legacy
   behavior remains compatible; `evidence_planner` fails closed until a real
   orchestration protocol exists.
6. The autonomy runner freezes distinct control/candidate binaries, records
   complete managed-server and per-policy provenance, counterbalances adjacent
   pairs, retains collision-safe traces, and excludes dirty or unpaired rows
   from model scoring.
7. Under the frozen Qwen control conditions, evidence mode clears the declared
   screening gate: at least one objective and contract completion, zero unsafe
   completions or mechanism violations, no more than the control's one
   unnecessary clarification, complete clean rows, and verified traces. If the
   first screen remains 0/3, at most two retained, trace-justified revisions to
   general controller behavior may be screened; each revision has distinct
   binary and trace provenance. A nonzero screen that fails a safety or
   clarification gate, or an exhausted revision budget without a qualifying
   candidate, is falsified. A qualifying candidate is frozen before paired
   confirmation and before any held task is inspected. It then either clears
   both promotion gates—including a positive held-task objective delta—or is
   recorded as falsified without substituting efficiency metrics for objective
   completion. With no qualifying candidate, confirmation and held tasks are
   explicitly skipped and the held-task seal is preserved. Every retained
   trace verifies and teardown is clean.
8. The planner arm receives an explicit evidence-based design or rejection;
   it never silently falls back to evidence-only, and the sprint closes with
   complete intent, work, test, and decision evidence.

## Rationale

The frozen control completed zero of three long-horizon objectives despite
valid constrained tool syntax. Traces showed blind overwrites, no-effect edits
counted as progress, unchanged failed checks, and recovery that preserved
transport but not a strategy grounded in current bytes. A causal controller at
the registry/loop chokepoint can make those failure modes structurally
unavailable and can be evaluated without changing the model or task prompts.

## Alternatives

- A task-specific prompt patch was rejected because it would not generalize and
  would contaminate the benchmark comparison.
- A larger model was rejected because the intent is to measure harness value at
  the pinned local-model tier.
- Planner-first orchestration was deferred because planning without trustworthy
  observations and effects would add another unverified state layer.
- Arbitrary shell execution was rejected in favor of operator-authored named
  checks that preserve the execution boundary.

## Consequences

Evidence mode has more trace events, checkpoints, tests, and conservative
blocks than legacy mode. Real-model validation is comparatively expensive and
must keep binaries, model identity, server topology, task corpus, and graders
fixed. Its structural safety and provenance improvements remain implemented,
but the frozen screen does not support a claim that this intervention improves
the pinned model's objective completion. Planner availability remains
deliberately absent: adding another state layer after the evidence-only
candidate failed its minimum gate would not be evidence-based. Any renewed
performance intervention or planner protocol requires a new intent rather than
rewriting this terminal result.

## Transition history

- 2026-08-02: created as `proposed` from the Sprint 113 research question.
- 2026-08-02: moved to `planned` when the user approved the engineering and verification contracts.
- 2026-08-02: moved to `active` when implementation began on the approved work packages.
- 2026-08-26: migrated into Book schema v2 as `active`; intent semantics and approved boundaries were preserved.
- 2026-08-26: revised acceptance criterion 2 after the migration audit found the legacy post-write Python warning could start an interpreter from the workspace; added an explicit no-implicit-execution boundary.
- 2026-08-26: clarified acceptance criterion 7 from the approved verification contract: at most two retained general revisions after a 0/3 first screen, candidate freeze before confirmation, and no held-task inspection before freeze.
- 2026-08-26: made the approved evaluation gates executable: unsafe or clarification-regressed screens cannot be selected, a no-candidate result follows an explicit skipped/falsified closeout path, and held-task promotion requires a positive objective delta.
- 2026-08-26: moved from `active` to `abandoned` after the frozen screen remained 0/3 objective-and-contract completions through both permitted general revisions; confirmation and held evaluation were consequently skipped, and the evidence did not justify implementing the planner arm.
