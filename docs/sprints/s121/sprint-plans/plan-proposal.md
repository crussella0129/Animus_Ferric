# Sprint 121 Plan Proposal — awaiting owner approval

Scratch Build/Test proposal, not either canonical plan and not a lock. Research
is committed at `13c7ccb`. The host exposes no `EnterPlanMode`/`ExitPlanMode`
tools; source remains unchanged. Owner approval, canonical critic and atomic
finalization are still required before Build. This is one sprint, not a new
application trial or a second review/refactor sprint.

## Build plan

### Intents and scope

- [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md), active: explicit-control/evidence portions of AC-11/12 only. T-11505 is the selected umbrella. Automatic speed/fit calibration, native reasoning, compaction tuning and application/skill verdicts remain open.
- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md), active: preserve AC-6/11/12 source execution, expert compatibility and human decision budget. No new first-run questionnaire, downloads, mandatory benchmarks or automatic tool-authority promotion.

Schema tree:

- Attributable explicit model budgets
  - T-12101: resolve and expose a context-reserve-bounded main-action cap.
  - T-12102: carry checked benchmark execution budgets and retained evidence.
  - T-12103: keep modified-budget measurements out of durable calibration.
  - T-12104: qualify the composed routes and document their narrow boundary.

### T-12101: Resolve one explicit main-action output cap

- **Intents:** INT-0007 AC-12 partial; INT-0008 AC-6/11/12 preservation.
- **Touches:** `crates/ferric-core/src/scale.rs`, `crates/ferric-cli/src/query.rs`, `crates/ferric-loop/src/run.rs`, additive trace types/tests in `crates/ferric-trace/src/event.rs` and `lib.rs`, corresponding query/loop/provider request tests. Mechanical optional/default field updates at existing constructors are allowed; no unrelated policy redesign.
- **Depends on:** none.
- **E01-A:** WHEN an explicit `query --max-output-tokens N` is supplied, THEN a shared resolver SHALL accept only a positive representable integer satisfying `N <= ctx.checked_sub(prompt_budget_tokens)` for the effective declared profile. Zero, integer overflow, invalid context/reserve and excess SHALL fail before provider construction, trace allocation, child launch or workspace mutation. Omission SHALL preserve existing tier caps, prompt budget and sampler behavior, including historical small-context behavior; no silent clipping or prompt shrinking.
- **E01-B:** WHEN a supported CLI query starts or resumes with an accepted cap, THEN main-action policy, sampler, actual HTTP `max_tokens`, and retained output-budget provenance SHALL agree. Requested value, effective value, declared context and source (`explicit` versus selected policy) SHALL be distinguishable from `tier_source`. Main-action request evidence SHALL report the actual sampler value even for a direct `RunArgs` caller whose nominal policy differs, without relabelling it as the policy default. Additive metadata SHALL retain old-trace readability.
- **E01-C:** WHEN a query with an explicit cap pauses and prints a resume command, THEN its existing shell-correct command SHALL repeat both that cap and its effective declared `--ctx` exactly. A manually invoked resume with the cap omitted SHALL retain existing fresh-per-segment budget selection; an explicit new valid cap SHALL apply to that invocation and be recorded. Resume SHALL revalidate against the newly selected policy and refuse an incompatible changed reserve before effects; it SHALL NOT copy prior tier authority, silently clip the cap, or introduce automatic whole-policy inheritance.
- **E01-D:** WHEN a bounded scripted provider returns a complete large constrained-JSON write under the accepted cap, THEN Ferric SHALL publish the exact complete file bytes through existing tool admission. WHEN either of two consecutive responses is marked truncated, THEN it SHALL dispatch no partial action; the existing one retry uses the same cap and the second truncation stops with the existing terminal reason.
- **E01-E:** WHEN an output override is applied, THEN tier, ring, tool/turn limits, controller/approval state, prompt reserve and the separate compaction sampler SHALL remain unchanged. This is a main-action cap, not a wall-clock or tokenizer-accurate fit guarantee.

Implementation notes: keep the shared resolver small and fallible. Do not add a
new human-facing knob to `ferric run`. Do not fix the entire direct-core policy
API under this task. Trace additions must distinguish actual main requests
from independent summarization. An actual wire observation, not a policy-only
assertion, proves propagation.

### T-12102: Apply checked benchmark agent budgets and retain attribution

- **Intents:** INT-0007 AC-11/12 partial; INT-0008 AC-6/11 preservation.
- **Touches:** `crates/ferric-cli/src/bench_cmd.rs`, `crates/ferric-bench/src/runner.rs`, new focused budget types/module if useful, `results.rs`, `summary.rs`, `provenance.rs`, `lib.rs`, and affected `Invocation`/`QuerySegmentRequest` constructor call sites in autonomy code/tests with default-preserving values. Existing process implementation/spec files remain unchanged.
- **Depends on:** T-12101.
- **E02-A:** WHEN `bench full --timeout-scale S` is admitted, THEN S SHALL be finite and strictly positive, with omission exactly 1.0. Each selected embedded agent timeout SHALL be converted once through checked multiplication/conversion to a positive representable duration before benchmark preflight effects, result/workspace creation or child launch. NaN, infinities, signed/unsigned zero, negatives, product overflow and positive underflow-to-zero SHALL fail without effects. Exact 1.0 preserves the embedded durations. Fractional durations SHALL be retained losslessly as seconds plus nanoseconds, not silently truncated to integer seconds.
- **E02-B:** WHEN benchmark output controls are selected, THEN both real and mock child routes SHALL receive the same effective context/parameters used for their output validation and attribution, and an explicit output cap SHALL occur exactly once in the initial and continuation argv. No new controls requested SHALL preserve frozen legacy continuation argv and default behavior. Provider-failure endpoint overrides, isolated profiles, `--no-config`, fixed temperature, protocol and workspace authority SHALL survive propagation unchanged.
- **E02-C:** WHEN an admitted benchmark run returns an outcome, THEN the parent SHALL retain base timeout, requested scale, exact enforced duration, requested cap, observed main-action cap/context when available, warmup `not_performed`, parent execution termination and the separate observed child/trace terminal cause. An early parent timeout SHALL remain attributable even with no child trace; missing observed values SHALL be null/explicitly unavailable, never invented defaults. Recording failure SHALL be an infrastructure failure, not a successful observation.
- **E02-D:** WHEN a trace is retained, THEN a versioned parent-authored budget sidecar SHALL bind its unchanged bytes with SHA-256 and share run/trial/level identity with its result row. Rows and summary SHALL carry the same budget controls and sidecar reference. Missing/malformed traces SHALL remain explicit evidence states. Both retained trace and sidecar SHALL use no-clobber/create-new publication: existing trace-only, sidecar-only or paired artifacts SHALL remain byte-for-byte unchanged on collision or write failure, including races. No successful observation SHALL be published until both newly owned files are complete and their binding verified; partial new evidence remains explicitly failed, not a valid pair. The parent SHALL NOT append synthetic completion or budget-enforcement claims to child traces. Legacy rows/summaries lacking metadata SHALL remain readable with unknown attribution, not fabricated scale 1.0.
- **E02-E:** WHEN scaled agent execution times out, THEN existing source-owned termination/draining/reaping SHALL complete before a successful runner return. The scale SHALL NOT modify post-run grader, startup, provider-independent fixture, capture or cleanup budgets. A returned provider error, output-limit stop and parent execution timeout SHALL retain different observed categories; no unsupported claim that a generic error was a provider deadline.

The parent sidecar supplies trace-bound deadline provenance because only the
parent enforces that deadline; the child's typed output/request records supply
the observed cap. No hidden child flag is needed to pretend it enforces its
parent's budget. Full endpoint discovery/fingerprinting remains T-12023/11507.
Explicit endpoints are used for qualification and still require separately
retained model/runtime identity. No new per-request HTTP timeout is added here.

### T-12103: Prevent diagnostic budgets from promoting durable capability

- **Intents:** INT-0007 AC-11/12 and diagnostic consequence; INT-0008 AC-6.
- **Touches:** `crates/ferric-bench/src/summary.rs`, `calibrate.rs`, `crates/ferric-cli/src/bench_cmd.rs`, benchmark unit/integration fixtures.
- **Depends on:** T-12102.
- **E03-A:** WHEN any explicit output cap or an effective non-default timeout scale is used, THEN the persisted summary and calibration evidence SHALL be marked diagnostic and calibration-ineligible with an explicit reason and no durable measured-level claim. Per-task outcomes, observed level statistics and failures SHALL remain intact. `calibrate_from_evidence` SHALL reject that evidence, not merely rely on the CLI skipping a write. Explicit scale 1.0 with no output override retains default calibration semantics.
- **E03-B:** WHEN a diagnostic single-model or fleet sweep finishes, THEN absent, valid, unrelated and malformed pre-existing profile data SHALL remain byte-for-byte unchanged; no profile SHALL be created or replaced. A diagnostic success/failure SHALL not become a persistence failure or advertised calibrated leaderboard entry. Default no-override calibration and existing partial-sweep behavior SHALL remain compatible.
- **E03-C:** WHEN human-readable results describe a timed-out, truncated, infrastructure-failed or diagnostic trial, THEN they SHALL distinguish the observed cause and evidence destination, not collapse the explanation into a model-capability verdict. Existing machine-readable compatibility fields SHALL remain available alongside the added classifications.

This task is a narrow eligibility/publication guard, not an atomic profile-store
repair. T-12023 remains open for all other writer paths and identity coordinates.

### T-12104: Qualify and document the composed budget increment

- **Intents:** INT-0007 AC-11/12 partial; INT-0008 AC-6/11/12 preservation.
- **Touches:** focused source-defined budget/live fixtures under `crates/ferric-cli/src/` and `tests/`; corresponding bench/loop/provider/trace tests; `docs/commands.md`, `docs/testbench.md`, narrowly touched benchmark comments/help; Book evidence. No new ad-hoc executable or process-control script.
- **Depends on:** T-12101, T-12102, T-12103.
- **E04-A:** WHEN an operator uses primary help or ordinary no-argument launch after this increment, THEN the existing compact human entry point and decision budget SHALL remain unchanged. Expert help SHALL explain main-action-only output scope, invocation-scoped resume behavior, scale effects, diagnostic profile preservation and evidence paths. Touched debug/Candle advice SHALL stop claiming external inference speed is determined by the Ferric build profile.
- **E04-B:** WHEN the opt-in live budget fixture runs against an existing repository GGUF and installed runtime, THEN it SHALL use the source-owned prepared-session path, an explicitly selected verified endpoint/model, a valid explicit main-action cap, retained request/trace/budget provenance and checked teardown. Fixture-local setup/request deadlines SHALL trigger cancellation with cleanup time reserved inside a source-supervised overall execution budget; stalled synchronous startup or request work SHALL not evade that outer owner. Deterministic stalled-phase fixtures SHALL prove cancellation and checked reaping. The smoke proves that path only, not a successful large action, hardware calibration, the frozen Qwen application or Sprint Loops support. Missing resources or failure are explicit non-passes, not silent skips.
- **E04-C:** WHEN this increment is proposed for merge, THEN fresh source-level focused/native tests and the canonical final-head CI matrix SHALL pass, and every EARS clause SHALL be mapped to actual named results. Loop SHALL reconcile remaining work, followed by the owner's extra independent adversarial audit before one dev-to-main PR; verify its final head/checks and stop for the owner to merge.

### Explicit deferrals

T-11506/11410/11412 app qualification; T-11507/12023 hardware, endpoint and
profile coordination; T-11508 reasoning/compaction; T-12110 oversized provider
usage conversion; T-12020 ICM ingress; T-12024 whole-Work/Git cancellation;
T-12026/27 native admission/parallel-load investigation; T-12028 Cargo warning.
No new downloads, tailnet mutations, blanket capability claim or source repair
of a model-authored app is included.

## Test plan

### Intent traceability

| Intent / affected acceptance | Task / clauses | Named verification |
|---|---|---|
| INT-0007 AC-12 partial; INT-0008 AC-6 | E01-A | `output_budget_default_matrix`, `output_budget_invalid_matrix`, `query_output_budget_rejects_before_effects` |
| Same | E01-B | `query_output_budget_request_trace_wire_agree`, `direct_request_budget_provenance_is_actual`, `legacy_budget_metadata_is_unknown` |
| INT-0008 AC-11 | E01-C | `query_output_budget_resume_guidance_roundtrip`, `query_resume_budget_is_invocation_scoped`, `query_resume_changed_reserve_rejects_before_effects` |
| INT-0007 AC-12 partial; INT-0008 AC-6 | E01-D | `large_action_budget_preserves_exact_bytes`, `truncated_large_action_never_dispatches` |
| INT-0008 AC-6/12 | E01-E | `output_override_preserves_authority`, `action_budget_does_not_retune_compaction` |
| INT-0007 AC-11 partial | E02-A | `timeout_scale_default_fractional_matrix`, `timeout_scale_invalid_matrix`, `bench_budget_rejects_before_effects` |
| INT-0007 AC-11/12 partial; INT-0008 AC-11 | E02-B | `bench_budget_argv_real_mock_and_resume`, `legacy_continuation_argv_unchanged`, `bench_mock_effective_context_matches_child` |
| INT-0007 AC-11 partial; INT-0008 AC-6 | E02-C/D | `bench_budget_trace_sidecar_roundtrip`, `bench_budget_pair_collision_preserves_evidence`, `bench_early_timeout_retains_parent_budget`, `bench_budget_recording_failure_is_infrastructure`, `legacy_budget_metadata_is_unknown` |
| Same | E02-E | `scaled_deadline_owns_checked_cleanup`, `benchmark_scale_leaves_grader_bounds_unchanged`, `benchmark_termination_causes_remain_distinct` |
| INT-0007 AC-11/12 partial | E03-A | `diagnostic_budget_evidence_cannot_calibrate`, `default_budget_calibration_compatible` |
| INT-0007 diagnostic consequence; INT-0008 AC-6 | E03-B | `diagnostic_single_fleet_preserve_profile_bytes`, `default_budget_calibration_compatible` |
| INT-0007 AC-11 partial; INT-0008 AC-11 | E03-C | `benchmark_termination_causes_remain_distinct`, `diagnostic_budget_operator_output` |
| INT-0008 AC-11/12 | E04-A | `budget_docs_preserve_human_front_door`, existing `human_cli`, `human_docs`, no-argument launch tests |
| INT-0007 AC-11/12 partial; INT-0008 AC-6 | E04-B | `real_model_explicit_budget_smoke`, `live_budget_fixture_stalled_phases_reap` |
| INT-0008 AC-6/11/12 | E04-C | focused result ledger, canonical Windows/Linux suites and final-head CI, Test critic, extra post-Loop audit |

### Unit tests

T-12101: table-test all six omitted tier defaults; explicit 1, exact remaining
reserve and reserve+1; zero, negative/oversized CLI spelling, zero/invalid
context and arithmetic edge values. Compare policy fields before/after and
capture actual main-action requests versus compaction requests. The loop
large-action fixture supplies a bounded 24 KiB payload with a deterministic
budget-sensitive provider: default cap yields truncated responses, accepted
4096-token cap at declared context 32768 yields the complete exact payload.
This is scripted token behavior, not a tokenizer or real-model benchmark.
Verify no partial file from either truncated response and the existing retry
count/stop reason. Exercise Legacy and Evidence paths where authority applies.

T-12102: scale cases include 1.0, 0.5, 2.0, fractional subsecond, NaN, both
infinities, positive/negative zero, negative values, overflowing product and
positive underflow-to-zero. Assert exact duration and unchanged verifier
timeouts. Pure argv tests cover real, mock, initial, resume, endpoint override,
hostile path spelling and absent-new-controls frozen argv. Metadata tests
cover valid/absent/malformed trace, mismatching digest, pre-existing sidecar,
sidecar-write failure and legacy rows/summaries. A dedicated publication matrix
injects trace-only, sidecar-only and paired collisions before and between
create-new operations; prior bytes must survive and a partial newly created
artifact must not become a successful observation. Do not infer observed caps
when a child never emitted them.

T-12103: construct complete full-ladder evidence so the eligibility tests cannot
pass merely because a mock failed L0. Both higher/lower scales and explicit cap
equal to a prior default remain diagnostic as specified; explicit scale 1.0
alone remains compatible. Existing successful default calibration must still
produce its previous tier behavior. Snapshot absent, valid multi-model and
corrupt profile bytes; test single/fleet success and failure without any write.
Complete synthetic full-ladder evidence must exercise the actual shared
publication decision used by both CLI paths, not a parallel test-only rule.
Label that success synthetic; real CLI mock/provider failure-preservation is a
separate integration result and is not a model success claim.

### Integration tests

- Drive real Cargo-built query CLI through finite local HTTP fixtures and
  assert exact serialized `max_tokens` for streaming and non-streaming calls,
  output-budget trace fields, unchanged guard authority and bad-input zero
  contacts/zero trace/workspace effects. Fixture threads use bounded I/O and
  joined cleanup; no sleeping daemon or ad-hoc helper executable.
- Execute printed resume argv through the existing source-owned shell fixture,
  asserting exact tokens including the explicit output cap, declared context
  and hostile path quoting. Include a cap valid at 32768 but not at default
  4096, then run the generated route with stable policy. Manually omitted/new
  explicit values exercise invocation scoping; changed policy/reserve must
  refuse before effects without copying tier authority or clipping.
- Drive `bench full --mock` plus scripted HTTP single/fleet variants through
  source-defined CLI fixtures. Read row, summary, sidecar and original trace
  bytes back; verify shared identity/digest/controls and profile preservation.
- Use finite source child modes under the existing process owner for early
  timeout before trace, timeout with partial trace, excessive captured output
  and normal return. Assert checked cleanup on each successful test return.
  Existing cleanup bounds, captures and CI schedule remain unchanged.
- Distinguish a controlled provider error/observed output limit from the
  parent timeout. Do not fabricate a new provider-deadline classification.

### End-to-end tests

**Status: possible for this partial increment.** The composed CLI/HTTP/trace/
result/profile lanes above are mandatory E2E evidence, not replaced by pure
argv assertions.

`real_model_explicit_budget_smoke` is an opt-in source-defined test using the
existing `FERRIC_LIVE_MODEL` convention and prepared-session ownership. Use an
already available model/runtime, declared context 4096 and a valid 1024-token
main-action cap for the 7B control, with a small constrained completion. Retain
actual model/runtime/endpoint/settings, response and trace; verify request
cap and cleanup. Run the live body as a source-defined child test mode under
the existing `ferric-process` owner, invoked only through its Cargo parent test.
Inside that body, arm a joinable cancellation watchdog for setup at 90 s and
bound the main request to 30 s; stop each watchdog promptly on phase completion.
The parent owns a 150 s execution budget independent of synchronous child
work, followed by the unchanged five-second checked process-scope cleanup.
Reserve the remaining margin within a 180 s acceptance ceiling for admission,
capture and ordinary teardown; an overrun is a failed test, never a successful
elapsed-only assertion. The actual bound is enforced by the parent owner, not
by extending the production 180 s startup default. Record parent execution,
phase cancellation and cleanup evidence separately. Do not claim a hard
real-time OS scheduling guarantee.

`live_budget_fixture_stalled_phases_reap` uses the same owner/cancellation
structure with shortened test-only phase budgets and finite source fixture
modes for stalled startup and stalled request. Each case must actually trigger
the intended cancellation/outer deadline and prove that all its children were
reaped. Unprovable cleanup is a test failure, never manual repair. No borrowed
or unrelated server may be stopped. No download/calibration is performed. The live test
must be run locally when resources are available; CI's lack of the model is
not a substitute for its result. If compatibility differs, classify and stop
acceptance rather than changing the model/limit silently.

The frozen larger-model medium-horizon app is **not yet this sprint's E2E**:
INT-0007 T-11506 → T-11410 → T-11412 owns its runtime requalification, no-repair
trial, grader and archive. Automatic measured-speed calibration/profile reuse
requires T-11507/T-12023, and independent thinking/compaction requires T-11508.

### Execution and review gates

Use source-aware Cargo commands only. After coherent edits, run formatter,
focused core/trace/bench/loop/CLI tests and provider wire tests, then full
workspace clippy/tests because the change crosses crate boundaries. Retain the
canonical Windows serialized workspace command and Linux namespace CI route,
backend-free CLI, lifecycle and ARM64 compile gates. Any included test module
must also be formatted. The final evidence ledger must state actual test names,
counts, outcomes, ignores with reasons, cleanup and immutable source head.

After accepted Test critique, reconcile intents/backlog and confidence once,
close Loop, obtain the separate fresh adversarial phase-completeness/code
review, then commit/push/confirm and open exactly one dev-to-main PR. Check the
current-sprint-only commit count and final-head CI; restore/hash-check the
protected Sprint 114 edit and stop for the owner's merge. A failed checkpoint
reopens this same sprint instead of creating a second PR or retrying to green.
