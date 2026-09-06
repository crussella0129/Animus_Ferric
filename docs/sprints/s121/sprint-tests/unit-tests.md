# Sprint 121 unit and clause evidence

Source head: `2856c63209865f69b3d3727f84fd92f63f9dfa51`.
Root's canonical Windows workspace run passed 1,299 tests with eleven
documented ignores. [Per-suite confirmations](windows-source-2856c63.txt) retain
the exact counts; they are not an acceptance override for the separate
[failed Windows CI checkpoint](ci-checkpoint-001.md). Formal Test is not passed.

The [sixteen-clause coverage index](coverage-ledger.md) maps the locked EARS
clauses to actual named tests and their source-backed assertions. This record
qualifies the unit/scripted portions; [integration](integration-tests.md) and
[live E2E](e2e-tests.md) are separate composed evidence.

## T-12101 — explicit action cap

- `output_budget_default_matrix` / `output_budget_invalid_matrix`: six omitted
  tier defaults, positive/exact reserve boundaries, excess/zero/overflow and
  invalid context; omission preserves historical small-context behavior.
- `direct_request_budget_provenance_is_actual`: actual sampler provenance
  replaces stale direct-caller metadata without inventing an explicit request.
- `large_action_budget_preserves_exact_bytes` /
  `truncated_large_action_never_dispatches`: deterministic 24 KiB payload;
  exact full publication versus no partial dispatch, existing retry and stop.
- `output_override_preserves_authority` /
  `action_budget_does_not_retune_compaction`: Legacy/Evidence authority fields
  and separate actual compaction sampler remain unchanged.

These are scripted loop providers, not tokenizer or real-model large-write
performance. Core units/snapshot passed 34+1; loop units passed 131 and all
five output-budget integration tests passed; trace units passed 34. Exact
other loop suite counts are retained in the canonical confirmation file.

## T-12102 — parent budget and evidence

The benchmark library passed 97 tests/four parent-entered source ignores.
Named tests cover positive finite/default/fractional duration matrices,
overflow and underflow-to-zero, real/mock/resume/default argv, mismatch before
workspace/child effects, source-owner early timeout, and unchanged grader
bounds. See `timeout_scale_default_fractional_matrix`,
`timeout_scale_invalid_matrix`, `bench_budget_argv_real_mock_and_resume`,
`legacy_continuation_argv_unchanged`, `bench_early_timeout_retains_parent_budget`,
`scaled_deadline_owns_checked_cleanup`, and
`benchmark_scale_leaves_grader_bounds_unchanged`.

`bench_budget_trace_sidecar_roundtrip`,
`bench_budget_pair_collision_preserves_evidence`,
`bench_budget_recording_failure_is_infrastructure`,
`budget_sidecar_rejects_tampered_metadata_and_malformed_json`,
`budget_observation_missing_malformed_and_future_vocabulary`, and
`legacy_budget_metadata_is_unknown` exercise unchanged bytes/digests/shared
identity, lossless duration, trace-only/sidecar-only/pair collisions and races,
partial-write refusal, malformed known vocabulary, future event compatibility
and unknown legacy attribution. No parent-authored child completion is invented.

## T-12103 — diagnostic publication guard

`diagnostic_budget_evidence_cannot_calibrate`,
`mixed_budget_controls_and_later_defaults_cannot_hide_diagnostics`, and
`default_budget_calibration_compatible` use complete synthetic successful
ladders, including mixed/forged derived flags, later default controls,
non-default scales both directions and explicit cap equal to an old default.
Direct library admission rechecks controls; raw outcomes remain intact.

`diagnostic_single_fleet_preserve_profile_bytes` exercises the actual shared
publication helper across successful/failed synthetic ladders and absent,
valid, unrelated, malformed and directory profile stores. Default successful
publication and partial-sweep preservation are positive controls. This is
explicitly synthetic success; actual scripted-provider CLI failure sweeps are
separate integration evidence, not a mock L0 substitute.

## T-12104 — bounded fixture evidence

The immutable CLI unit suite passed 389 tests/four documented ignores locally.
`live_budget_fixture_stalled_phases_reap` proves actual setup/provider
cancellation and independent synchronous-stall outer timeouts with checked
cleanup. Five further tests cover known SHA vectors/chunks, refusal before
read, cancellation between chunks, cancellation after read before update and
raw partial stage-journal retention. Both ordinary debug and release Build
gates passed these six tests; the fresh immutable full workspace executed
them again. The two opt-in real-model tests are not silently treated as runs;
the explicit-budget test was separately executed and recorded in E2E evidence.

## Intent boundary

INT-0007 AC-11/12 advance only explicit controls and attribution, not measured
speed, hardware fit, reasoning or compaction tuning. INT-0008 AC-6/11/12 are
preservation requirements: bounded source ownership, expert compatibility and
the existing small human front door. The Windows CI first-run failure leaves
that composed acceptance unproved until diagnosed and requalified; neither
intent is realized and no final Test verdict is inferred from unit totals.
