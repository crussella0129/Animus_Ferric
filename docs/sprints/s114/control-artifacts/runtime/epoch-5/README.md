# Epoch-5 publication correction

Epoch 5 corrects one post-copy wrapper defect in the frozen epoch-4
publisher. It does not run a model, change the Q4/32K coordinate, alter any
epoch-3 measurement byte, or rewrite an epoch-4 control. The execution
identity remains `e03-01-q4-32768`; epoch 5 owns only the separately frozen
publication correction.

Epoch 4 successfully froze its recovery controls and copied the exact source
tree into an isolated stage. Its publisher then failed closed while checking
the stage verifier report: the wrapper compared attestation fields against
`$plan.template_attestation` and `$plan.process_command_attestation`, but
`$plan` was the epoch-4 recovery plan rather than the anchored epoch-3 source
runtime plan. The stage was never promoted. The destination and legacy
epoch-4 publication envelope therefore remained absent, and the immutable raw
source remains the authority. `incident.json` records that boundary.

`runtime-plan.json` anchors the frozen epoch-4 controls and failed publisher,
the epoch-3 source plan, the exact raw manifest and selected terminal
artifacts, and the Q4 model identity. Epoch 5 uses the already frozen epoch-4
verifier with the correct source-plan binding. The corrected publisher copies
the exact 49-entry raw tree, emits the legacy
`epoch-4/recovery-publication.json` envelope required by the frozen downstream
gate, and records the separately controlled correction in
`epoch-5/publication-correction.json`.

Run the correction chain in this order:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-5/test-publication.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-5/freeze-publication.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-5/publish-e04-correction.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/record-q4-verdict.ps1 -AttemptId e03-01-q4-32768
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/verify-q4-gate.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/finalize-selection.ps1
```

Do not edit files or change `HEAD` between epoch-5 freeze and completion of
the publication, verdict, gate, and finalization chain. Freeze requires an
exact green epoch-5 self-test, the exact frozen epoch-4 control set and all
anchored dependencies, the exact raw source tree, a cold Ferric/server state,
absence of the destination and both publication evidence files, and an
independent live Q4 rehash. The publisher never overwrites differing evidence
and only resumes an existing destination when every byte is the frozen source
tree.

Epoch 4 remains immutable incident evidence. Do not patch, regenerate, or
replace its frozen publisher or controls. Its generated legacy envelope is an
authorized output of the epoch-5 correction, not an epoch-4 control rewrite.
