# Sprint 114 runtime calibration — control epoch 3

This directory is the third, separately frozen control epoch for T-11409.
The first two attempts remain immutable evidence:

- Epoch 1 is the parent `runtime/` control and archived
  `attempts/01-q4-32768`. It exposed invalid assumptions about omitted
  `/props.default_template_kwargs` and verifier environment reconstruction.
- Epoch 2 is `runtime/epoch-2/` and archived
  `attempts/e02-01-q4-32768`. Its four-arm `/apply-template` differential,
  child-`PATH` reconstruction, model identity, CUDA startup, endpoints, and
  teardown all passed. Its terminal `infrastructure_blocked` verdict came
  from one control defect: both attestors replaced Ferric's observed bare
  `llama-server` argv[0] with WMI's absolute `ExecutablePath`.

Epoch 3 fixes that second control defect. It parses the retained command line
with Windows `CommandLineToArgvW` semantics, independently binds the captured
absolute executable path and live SHA-256 to the frozen image, accepts only
the declared bare `llama-server` token or that exact frozen absolute path as
argv[0], and compares every remaining argument ordinally with exact count and
boundaries. It also normalizes process creation to UTC and binds it to the
preflight-to-attestation launch window and retains the empty listener/process
snapshots behind its cold-state claims. Epoch 3 transitively validates both
prior control manifests
and exact attempt trees, preserves their Git `-text` policy, and covers epochs
1, 2, and 3 in the final artifact manifest.

The model, llama.cpp b10516 CUDA runtime, prompts, template-probe bytes,
sampling, limits, retry gates, and throughput rules are unchanged from epoch
2. The `E02` strings inside the four template requests are intentionally
carried-forward sentinels whose exact bytes are part of that continuity
contract.

## Evidence locations

Raw epoch-3 attempts are created under
`target/s114-experiment/runtime-epoch-3/smoke/`. Only after the managed server
is torn down and every retained file is sealed does `run-coordinate.ps1` copy
an attempt byte-for-byte into `epoch-3/attempts/`. Machine-specific paths may
occur only in retained raw evidence; the live template surface remains
generic.

## Controlled run sequence

Run the complete self-test once, before freezing:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-3/test-runtime.ps1
```

Freeze the inputs from the required cold GPU/server state:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-3/freeze-runtime.ps1
```

Then run the primary coordinate:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-3/run-coordinate.ps1 `
  -Coordinate e03-01-q4-32768
```

Do not commit, amend, switch branches, or otherwise change repository HEAD
between freeze and completion of the authorized coordinate chain. Both freeze
and launch require the exact pre-control checkpoint recorded in
`runtime-plan.json`. A 16K retry is legal only after a verified
`startup_memory_pressure` result; Q3 remains illegal until the verified Q4
gate explicitly authorizes it.

After the terminal Q4 attempt, publish and independently verify its gate:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-3/record-q4-verdict.ps1 `
  -AttemptId e03-01-q4-32768

pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-3/verify-q4-gate.ps1
```

Use `e03-02-q4-16384` instead only when that retry was authorized and became
the terminal Q4 attempt. Once every authorized attempt is complete, finalize
the selection and all-epoch coverage manifest:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-3/finalize-selection.ps1
```

If the Q4 gate authorizes Q3, complete its declared chain first and pass the
terminal Q3 attempt with `-Q3AttemptId`. A non-viable coordinate is an
experimental result; evidence corruption, invalid authorization, incomplete
teardown, or a wall-cap breach is infrastructure failure.
