# Sprint 114 runtime calibration

This directory freezes and retains the T-11409 managed-server calibration for
the selected Qwen3.8-27B GGUF. `runtime-plan.json`, `nonce.txt`,
`smoke-prompt.txt`, and `throughput-request.template.json` are operator-owned
inputs. They must not change after `control-inputs.json` is generated and
before every declared coordinate has terminated.

Preflight found that the installed llama.cpp b10034 executable had only CPU
backend DLLs: `--list-devices` returned no device even though the operating
system exposed the NVIDIA GPU. A matching b10034 CUDA bundle was verified, but
additional pre-inference research found an upstream Qwen3.8 report in which an
older CUDA DeltaNet path loaded normally and generated corrupt tokens; the
report identified a working update around b10450. Because b10034 predates that
fix, using it would risk misclassifying a known engine defect as model quality.

The calibration therefore pins the then-current official b10516 CUDA 12.4
release, verified before any inference, and prepends its ignored directory to
the managed child's `PATH`. It does not replace the machine-wide installation
or bypass `ferric server up`. The compatibility decision is an inference from
the upstream report and the release ordering, not a claim that every Qwen3.8
engine defect is fixed.

- Official release: <https://github.com/ggml-org/llama.cpp/releases/tag/b10516>
- Upstream DeltaNet report:
  <https://github.com/ggml-org/llama.cpp/discussions/27164>

Raw attempt directories are created under `target/s114-experiment/smoke/`.
Only after a managed server is torn down and the evidence is sealed does
`run-coordinate.ps1` copy that attempt byte-for-byte into `attempts/` here.
Machine-specific paths may occur only inside those retained raw evidence
files. Requested settings and observed effective settings are always distinct.

Run the control self-test before inference:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/test-runtime.ps1
```

Freeze the operator inputs next:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/freeze-runtime.ps1
```

The primary coordinate is then invoked with no tunable arguments:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/run-coordinate.ps1 `
  -Coordinate 01-q4-32768
```

The script refuses an undeclared coordinate, an unauthorized context retry,
or Q3 without a verified Q4 non-viability gate. A non-viable model coordinate
is an experimental result rather than an orchestration error; evidence
corruption, incomplete teardown, or a wall-cap breach is infrastructure
failure.
