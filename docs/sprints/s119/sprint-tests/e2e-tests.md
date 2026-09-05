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

Final corrected-head results and phase/remote receipts remain pending.
