# Sprint 113 Plan Readiness — Evidence-Bound Recovery

## Decision

The real Qwen control is usable as a diagnostic floor, and the codebase has a
coherent seam for a general intervention. Sprint 113 should first implement and
screen an evidence/repair controller. Planner orchestration is a second,
separately measured arm and must not be mixed into the evidence-only result.

No tracked implementation starts until the owner approves the engineering
contract and its repository-local build and verification specifications.

## Frozen comparison

- Baseline commit: `cabe2368154339013c39958da43580db86e19f78`
- Frozen control executable:
  `sprints/s113/control-artifacts/ferric-control-cabe236.exe`
- Control tasks: H01, H04, H08; `recovery`; one trial; grammar protocol
- Control result: 0/3 contract passes and 0/3 objective completions
- Model: exact pinned Qwen2.5-Coder-7B Q4_K_M artifact
- Context/topology: 8,192 tokens, managed local `llama-server`, CPU

The 0/3 result is not a population-accuracy estimate. It is a reproducible
diagnostic baseline for causal comparison on the same coordinates.

## Architecture decision

### Orthogonal harness policy

Add a `HarnessPolicy` distinct from autonomy task `variant` and from the tool
permission policy:

- `legacy`: current behavior and rollback path
- `evidence`: observation, effect, verification, and recovery controller
- `evidence_planner`: later planner-plus-evidence experimental arm

Record the selected policy in the trace and benchmark provenance. Existing
traces and rows deserialize as `legacy`. A resumed session must keep its
original harness policy.

### Typed tool-control seam

Do not infer safety-relevant effects from tool names or English result strings.
Extend the tool/registry boundary with runtime-only semantic metadata and a
prepared-effect path:

1. Resolve path, ignore, permission, and command-policy checks.
2. Prepare a side-effect-free observation, named check, or exact mutation
   candidate.
3. Let the evidence controller admit or reject it.
4. Request human approval only for an admitted action.
5. Commit the prepared candidate.
6. Return typed execution effects independently from textual tool output and
   independently from success/error status.

Built-in file readers and content mutators opt into precise classifications.
Unknown write/execute tools default conservatively to opaque mutation behavior.

### Evidence ledger

For each observed file, retain:

- canonical internal path and normalized workspace-relative display path
- full-file SHA-256
- total lines/bytes
- untruncated line ranges actually shown to the model
- observation turn/sequence and whether the model view is complete

`read_file` renders an envelope before the content containing path, returned
range, total size, completeness/truncation, and fingerprint. The controller
uses typed metadata rather than parsing that envelope.

Rules:

- An existing file requires complete current evidence before any full write,
  edit, multi-edit, or patch in this sprint's conservative implementation.
- Partial, non-truncated reads may accumulate only while the full-file hash is
  unchanged.
- A read and mutation proposed in the same native multi-call turn do not
  authorize one another; the model chose both before seeing the read result.
- The hash is checked again immediately before commit. Drift blocks the action.
- A genuinely absent file may be created without prior file evidence.
- A successful content mutation establishes authored evidence for the new
  bytes, but a failed named check requires a later-turn reread before repair.
- Resume invalidates file freshness and requires rereading.

### Effect-aware mutation and syntax admission

Content tools prepare candidate bytes in memory and compare them with current
bytes. Identity replacements, identical writes, and net-zero multi-edit/patch
operations are rejected as `no_effect`; they do not advance mutation epochs.

Supported syntax checks run on candidate bytes before commit. Python uses
`ast.parse` through stdin. Admission is:

| Existing state | Candidate state | Decision |
| --- | --- | --- |
| valid | valid | allow |
| valid | invalid | block atomically |
| invalid | valid | allow |
| invalid | invalid but changed | allow with explicit warning |
| absent | invalid | block atomically |
| unsupported/unavailable validator | unknown | allow and record not checked |

The invalid-to-invalid case permits incremental repair without trapping a
repository that was already broken.

Mutation epochs advance only from measured workspace effects. Error status and
effect status remain separate because a tool can fail after a partial change.
Workspace fingerprints exclude `.git` and `.ferric`; VCS metadata must not
manufacture a source mutation.

### Verification repair controller

On a failed named check, record the check name, current mutation epoch, attempt,
failed turn, and a SHA-256 of normalized full diagnostics. Normalize line
endings, workspace prefixes, and trailing whitespace before hashing.

- The same check at the same mutation epoch is blocked before spawning another
  process.
- A content repair requires a fresh eligible observation from a later model
  turn after the failure.
- A real mutation permits the check again.
- Controller blocks return ordinary model-facing tool errors and remain
  bounded by existing failure guards.
- Metrics distinguish intercepted attempts from real tool/check executions.

### Trace and recovery state

Extend the central `TraceStructure` state machine rather than building a second
validator in the CLI. Add typed events for file observation, controller block,
mutation effects, failed checks, repair attempts, controller checkpoints, and
recovery packets. Explicitly validate these events; do not let the current
wildcard silently accept them.

Keep legacy recovery checkpoints compatible. Evidence-policy sessions emit a
separate versioned controller checkpoint on fresh start and at pause/resume
boundaries. Legacy traces remain valid without it; evidence traces require it.
Controller checkpoints contain repair facts but resume conservatively marks
file observations stale.

Every non-clarification continuation receives a deterministic, machine-derived
recovery packet containing pause reason, mutation epoch, required/passed
checks, last failed check/fingerprint, and reread requirements. The packet is
projected to the model, while authoritative state remains outside model history
so compaction cannot rewrite it.

### Planner correction

The existing `ActionProtocol::Plan` is a terminal whole-session protocol:
`submit_plan` ends successfully, successful sessions cannot resume, resume must
keep the same protocol, and autonomy currently expects one trace per segment.
Therefore `RunPolicy.uses_planner` cannot simply be wired into execution.

The sprint proceeds in this order:

1. Implement and screen `evidence` against `legacy`.
2. Only if the evidence mechanism works, freeze a planner-arm design before
   building it.
3. Prefer two explicitly linked sessions: a bounded read-only planner trace and
   an execution trace that receives a validated structured plan. Extend runner
   trace discovery and provenance accordingly.
4. If linked sessions prove incompatible with resume/benchmark invariants, use
   an embedded, checkpointed planner-to-executor phase machine instead. Do not
   silently fall back to evidence-only in a measured planner arm.

The plan must name observed targets/fingerprints, invariants, ordered steps, and
the final named check. Existing targets must have planner observation evidence.

## Results and runner migration

Add row/provenance fields and counters with backward-compatible defaults:

- harness policy
- eligible observations
- blind/stale/repair-inspection blocks
- no-effect and syntax blocks
- failed-check attempts/fingerprints
- unchanged-check blocks versus actual check executions
- repair attempts and recovery packets
- planner transitions/linkage if the planner arm is built

Use the candidate benchmark runner to interleave arms per task/trial on the same
managed server. It may invoke the frozen executable for `legacy` without new
flags and the candidate executable for `evidence`. The candidate runner owns
task creation, grading, aggregation, and arm labels, avoiding a requirement for
the old executable to understand the new results schema.

Rotate arm order per coordinate. Report paired outcomes (legacy only, evidence
only, both, neither) as well as per-arm totals. Preserve raw rows and verify all
retained traces.

## Reproducibility correction

The frozen autonomy runner sends temperature `0`. llama.cpp defines temperature
zero as greedy selection, so the server's default random seed (`-1`) did not
turn the control into a stochastically sampled run. The 0/3 result is diagnostic
because it is one pass over three selected tasks, not because its sampler was
random.

Before paired confirmation, expose and record managed-server seed and slot
count, use one slot, and retain temperature/top-p plus engine build provenance.
This hardens reproducibility and future-proofs nonzero-temperature experiments.
Three same-setting trials are reproducibility/stability repeats, not independent
Bernoulli draws; do not use them to imply population confidence.

## Build order

1. Harness policy and backward-compatible trace/result wire types.
2. Typed registry preparation/effect contracts and navigation envelopes.
3. Candidate syntax validation, no-effect rejection, and file evidence ledger.
4. Trace projection/structure, controller checkpoints, replay, and recovery
   packets.
5. Live loop admission, failed-check controller, and actual-effect epochs.
6. `trace verify`/`trace cat`, query/runner/autonomy plumbing, metrics, paired
   aggregation, and full sampling/server provenance.
7. Unit, loop, replay, malformed-trace, CLI, and benchmark integration tests.
8. Real evidence-only screening and paired/held-task E2E evaluation.
9. Planner-arm design/build only after the evidence screen and a frozen design
   addendum.

## Evaluation schedule and gates

### Screen

Run H01/H04/H08 once with `evidence` on the exact control model/topology.
Continue to paired confirmation only if at least one objective completion is
observed and the mechanism/safety gates pass.

### Paired confirmation

Run `legacy` and `evidence` for three interleaved trials each on H01/H04/H08
(18 rows total) with the same deterministic sampling/server policy. Treat the
repeats as stability evidence. Require a positive paired completion delta and
at least one task completed by evidence in at least 2/3 repeats.

### Held tasks

Run H02/H03/H05/H06/H07 once per arm after the paired confirmation. Treat this
as a generalization check, not enough data for a broad population claim.

### Non-negotiable mechanism/safety gates

- zero admitted blind or stale existing-file content mutations
- zero admitted no-effect content mutations
- zero repeated named-check process executions at the same mutation epoch
- no arbitrary execution or widened tool ring
- complete, infrastructure-clean result sets
- every retained trace passes side-effect-free structural verification
- server launch/status/query/down independently validates the exact model and
  leaves no matching process/listener behind

Intercepted controller blocks may be nonzero; they are evidence that the
mechanism caught an attempted violation.

## Principal risks

- A partial retrofit that keys on tool names or English output can make the
  benchmark look better while custom/aliased tools bypass the invariant.
- Treating successful calls as mutations corrupts check freshness; actual
  effects must drive epochs.
- Same-turn multi-call authorization leaks information the model did not have
  when it proposed the mutation.
- Adding safety-critical optional fields only to the legacy checkpoint lets old
  binaries ignore the controller; a distinct required controller checkpoint
  avoids that ambiguity.
- Planner work can dominate the sprint without testing the causal evidence
  hypothesis. It stays behind the evidence-only screen.
- Three selected development tasks do not estimate a broad coding-task
  population even when their greedy trajectories are repeatable; untouched
  held tasks remain mandatory.
