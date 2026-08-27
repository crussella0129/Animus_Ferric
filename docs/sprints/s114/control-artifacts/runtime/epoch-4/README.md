# Epoch-4 exact-byte recovery

Epoch 4 publishes the already completed epoch-3 Q4/32K measurement. It does
not execute a model, change the coordinate, or reinterpret the attempt as an
epoch-4 run. The execution identity remains `e03-01-q4-32768`; epoch 4 owns
only the recovery and publication protocol.

The epoch-3 attempt reached a terminal `viable` verdict with complete evidence
and a proven teardown. Its first publication failed after measurement because
PowerShell deserialized RFC 3339 strings as local `DateTime` values and a
later string cast discarded the round-trip representation. The corrected
verifier uses the frozen
`powershell-json-datekind-string-rfc3339-v1` protocol: JSON dates stay strings,
timestamp parsing requires an explicit offset, and comparisons use normalized
UTC instants.

`raw-source-anchor.json` fixes the source directory's exact 49-file payload,
its manifest, selected terminal artifacts, and byte counts. `runtime-plan.json`
also anchors the immutable epoch-3 controls and the Q4 model identity. Neither
file authorizes changes to any epoch-3 control or source byte.

Run the recovery chain in this order:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/test-runtime.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/freeze-runtime.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/recover-e03-publication.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/record-q4-verdict.ps1 -AttemptId e03-01-q4-32768
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/verify-q4-gate.ps1
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-4/finalize-selection.ps1
```

Do not edit files or change `HEAD` between freeze and completion of the
recovery, gate, verdict, and finalization chain. Freeze requires the exact
epoch-3 source, green epoch-4 self-tests, a successful corrected semantic
verification, a cold Ferric/server state, and an independent Q4 hash. The
publisher never overwrites evidence. An existing destination is accepted only
when every byte is already the exact frozen tree, allowing safe recovery after
an interruption.

The recovered attempt is published at
`docs/sprints/s114/control-artifacts/runtime/epoch-3/attempts/e03-01-q4-32768`.
Its external publication envelope is `recovery-publication.json` in this
directory. Keeping the envelope outside the attempt preserves exact-byte
identity and the original 49-entry manifest.
