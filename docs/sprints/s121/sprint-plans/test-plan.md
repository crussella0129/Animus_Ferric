Finalized - DO NOT EDIT

# Sprint 121 Test Plan

## Intent Traceability

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

## Unit Tests

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

## Integration Tests

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

## End-to-End Tests

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

## Execution and Review Gates

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
