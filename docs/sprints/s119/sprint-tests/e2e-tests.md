# Sprint 119 End-to-End Verification

## Scope

Model-free command and lifecycle execution is possible and required. CLI/bench
tests exercise real Cargo-selected command entry points with observable outputs;
managed lifecycle fixtures drive `up`, `status`, `down`, legacy adoption and
scoped fake LocalAPI ownership. Mocks do not satisfy live-tailnet or real-model
application acceptance. INT-0007 tasks T-11505/T-11506 and T-11410/T-11412 still
unlock the separately frozen real-model application trial.

## Retained first committed attempt

At `712e3cc5eae19170601d3c3feaee4deab03bbbd4`, Windows lifecycle passed 5/5.
Linux lifecycle failed 3/6 under `bash tools/test-lifecycle-linux.sh`, which ran
Cargo inside the non-root PID/network namespace and propagated exit 101.
The failed source topology and required correction are retained in
[Test corrections](../test-phase-corrections.md). Existing production
ownership refusal was correct and remains unchanged.

## Clause and phase boundaries

- E04/E05 require the corrected native positive suite plus exact-owner-death
  regression, not merely the passing source-text CI ratchet.
- E07 combines `source_driven_ci_contract` with the actual isolated Linux Cargo
  job. No executable extraction/direct target launch is permitted.
- E08 `sprint_phase_and_remote_audit` is an offer-for-merge gate. Test supplies
  clause evidence and an independent critic; Loop supplies reconciliation,
  validation and closure. The additional independent post-Loop audit must pass
  before the sole dev-to-main PR is created. Remote SHA/base/head/count checks
  must then confirm that actual PR before handoff. A Test report does not
  pretend those future Loop/remote actions have already occurred.

Final corrected-head results are recorded below. Only E08's subsequent Loop
and actual remote receipts remain pending at this Test-phase boundary.

## Corrected intermediate native result

At `0776b6d986ba3852aba14b1742926a9a90343f9d`,
[CI run 33935599036](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935599036)
passed all six jobs. Linux's source-wrapper suite passed these six tests:
`legacy_adoption_then_down_cli_e2e`,
`lifecycle_fixture_exits_when_exact_owner_pidfd_signals`,
`model_free_server_lifecycle_fixture_e2e`,
`ordinary_ferric_ignores_lifecycle_localapi_override`,
`tailscale_localapi_lifecycle_preserves_unrelated_state`, and
`tailscale_localapi_log_contains_no_broad_mutation_or_retry`.
Windows passed the five applicable tests (the pidfd test is Linux-only).
These assertions include CLI outputs, original ownership refusals, exact
teardown, registration restoration, unrelated-state preservation, and absence
of broad mutation/retry. The real launcher is now reaped before status is
published, while its source supervisor remains the live owned group anchor.

## Final source-head result

Head **`81c9aeaf0a9c08f8909395d77a6c7bd53204ee94`** passed all six jobs in
[CI run 33935893263](https://github.com/crussella0129/Animus_Ferric/actions/runs/33935893263).
The same six named Linux lifecycle tests passed **6/6 in 5.32s** and the five
Windows tests passed **5/5 in 19.10s**. Command integration (`cli` 68 tests,
`bench_mock` 7) and source-CI ratchet also passed in both workspace jobs.
Thus E04/E05/E07 have final-head native evidence, including the repaired live
supervisor and preserved exact-reaping/ownership assertions. The Windows-only
deadline correction also passed the full native fixture suite.

E08's remaining Loop and actual remote receipts must be checked after the
independent Test critique and before handoff; the closing audit will link back
to these results. There is no claim of a live model app, real tailnet exercise,
macOS parity, ordinary-host Linux listener-owner exclusivity, general group
escape containment, or completion of the entire INT-0008 workflow.
