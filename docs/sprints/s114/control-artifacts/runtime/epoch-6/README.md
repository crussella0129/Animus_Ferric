# Epoch-6 evidence materialization

Epoch 6 repairs only the last evidence-materialization boundary left by the
immutable epoch-5 publisher. It does not run a model, copy or move the
published destination, change the Q4/32K coordinate, alter an epoch-3
measurement byte, or patch an epoch-4 or epoch-5 control. The execution
identity remains `e03-01-q4-32768`.

Epoch 5 froze successfully and atomically published the exact 49-entry source
tree to `epoch-3/attempts/e03-01-q4-32768`. Its publisher then failed closed
before writing either evidence envelope. The frozen validator was called on a
freshly constructed `OrderedDictionary`; its PSObject property enumeration did
not expose the ordered JSON keys, so an otherwise intended 14-field legacy
envelope was rejected. `incident.json` records the exact failure and the
post-failure boundary. Epoch 5 remains immutable incident evidence.

`runtime-plan.json` anchors the frozen epoch-5 plan, controls, self-test,
shared implementation, and failed publisher; the complete frozen epoch-4 and
epoch-3 dependency chain; the exact raw source and published destination; and
the retained Q4 model. Freeze rewalks those controls, proves source and
destination byte identity, requires all three declared outputs to be absent,
requires a cold runtime, and invokes the frozen epoch-4 verifier once against
the existing destination. The full verified report is frozen into the epoch-6
control manifest for deterministic reuse.

The first pre-control epoch-6 self-test completed its single model hash but
failed one harness-only assertion: a global text search selected the earlier
resume-path validator instead of the validator after the frozen ordered
constructor. No control or official output existed. Its exact 39,692 bytes are
preserved as `materialization-self-test.failed-01.json`, anchored by the plan,
incident, corrected self-test, freeze, and materializer. The corrected harness
searches only after the constructor and does not reinterpret or edit that
failed report.

The materializer uses that report to create the legacy epoch-4 envelope and
epoch-5 correction evidence without republishing the destination. It then
executes the frozen epoch-5 publisher as an authoritative read-back check and
records that outcome in `materialization.json`. It never overwrites a
differing destination or evidence file and only resumes states explicitly
covered by the frozen materialization state machine.

Run the materialization chain in this order:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-6/test-materialization.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-6/freeze-materialization.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-6/materialize-e05-evidence.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/record-q4-verdict.ps1 -AttemptId e03-01-q4-32768
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/verify-q4-gate.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/finalize-selection.ps1
```

Do not edit files or change `HEAD` between epoch-6 freeze and completion of
the materialization, verdict, gate, and finalization chain. Do not regenerate,
patch, or replace any frozen epoch-3, epoch-4, or epoch-5 artifact. The legacy
epoch-4 envelope and epoch-5 correction file are authorized outputs of the
separately frozen epoch-6 repair, not rewrites of earlier controls.
