# Sprint 114 runtime calibration — control epoch 2

This directory is the separately tested and frozen recovery control for
T-11409. The parent `runtime/` directory and its archived `01-q4-32768`
attempt are immutable epoch-1 evidence. Epoch 2 anchors those exact bytes,
uses distinct coordinate/archive/raw names, and changes only two invalid
experimental attestation assumptions discovered after the first launch. It
also hardens evidence integrity by rejecting unlisted attempt files, anchoring
Git's binary-preservation rule, comparing stable parsed device identity rather
than volatile free-VRAM text, and covering both runtime epochs at finalization.

llama.cpp b10516 intentionally omits launch `default_template_kwargs` from
`/props`. Epoch 2 therefore proves effective reasoning preservation with a
four-arm, non-inference `/apply-template` differential bound to the exact
served chat-template hash. It also reconstructs the child `PATH` from the
parent `PATH` retained at launch rather than depending on the environment of a
later verifier process. The model, binary, runtime parameters, prompts,
sampling, fallback gates, and throughput rules are unchanged.

`runtime-plan.json`, `nonce.txt`, `smoke-prompt.txt`, the four
`template-probe-*.json` requests, and `throughput-request.template.json` are
operator-owned inputs. They must not change after `control-inputs.json` is
generated and before every declared epoch-2 coordinate has terminated.

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

Raw attempt directories are created under
`target/s114-experiment/runtime-epoch-2/smoke/`.
Only after a managed server is torn down and the evidence is sealed does
`run-coordinate.ps1` copy that attempt byte-for-byte into `attempts/` here.
Machine-specific paths may occur only inside those retained raw evidence
files. Requested settings and observed effective settings are always distinct.

Run the control self-test before inference:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-2/test-runtime.ps1
```

Freeze the operator inputs next:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-2/freeze-runtime.ps1
```

The primary coordinate is then invoked with no tunable arguments:

```powershell
pwsh -NoProfile -File docs/sprints/s114/control-artifacts/runtime/epoch-2/run-coordinate.ps1 `
  -Coordinate e02-01-q4-32768
```

The script refuses an undeclared coordinate, an unauthorized context retry,
or Q3 without a verified Q4 non-viability gate. A non-viable model coordinate
is an experimental result rather than an orchestration error; evidence
corruption, incomplete teardown, or a wall-cap breach is infrastructure
failure.
