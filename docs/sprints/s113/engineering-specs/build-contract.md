# Sprint 113 Approved Build Contract — Evidence-Bound Autonomous Recovery

Approved by the user on 2026-08-02. This contract is repository-native,
tool-independent Markdown. It is a reference specification, not an IDE
artifact or executable workflow. Corrections discovered during implementation
must be recorded in a dated addendum, not by rewriting this baseline.

## Fixed experiment

- Preserve the exact Qwen2.5-Coder-7B GGUF, model hash, context, constrained
  protocol, temperature-zero sampling, corpus, graders, and server topology.
- Preserve the frozen `cabe2368` control executable and never pass it flags it
  does not understand.
- Keep task variant and harness policy as orthogonal dimensions.
- Admit no task-specific prompts, arbitrary shell widening, or offline evidence
  in place of the real managed-server comparison.

## B113-01 — Policy and additive wire foundation

- Add `HarnessPolicy::{Legacy, Evidence, EvidencePlanner}` in `ferric-core`,
  defaulting old serialized data to `Legacy`.
- Record policy selection independently from autonomy task variants and reject
  explicit policy changes on resume.
- Add versioned observation, controller-block, workspace-effect, verification,
  controller-checkpoint, and recovery-packet trace payloads.
- Keep old trace schema/checkpoint fixtures readable and make old binaries fail
  closed on evidence-only events.
- Extend event rendering and the central `TraceStructure`; do not duplicate new
  safety truth in `trace_verify`.
- Gate: literal legacy fixtures round-trip and malformed event ordering fails.

## B113-02 — Typed tool preparation and measured effects

- Add a typed control seam to `ferric-tools` that can prepare a model-visible
  operation without mutating, describe required observations, and commit only
  after controller/approval admission.
- Preserve the existing `Tool::run` and registry execution path for legacy and
  direct-human use.
- Envelope reads with normalized path, requested/returned range, total size,
  truncation/completeness, and full-content SHA-256.
- Make literal find/search zero results explicit with typed metadata.
- Prepare candidate bytes for write/edit/multi-edit/patch and classify actual
  create/modify/delete/no-effect outcomes.
- Revalidate hashes immediately before commit; fail closed for opaque model
  mutations and structural operations without complete semantics.
- Block valid-to-invalid and absent-to-invalid supported syntax transitions;
  allow materially changed invalid-to-invalid repairs with a warning.
- Gate: tool/registry tests cover empty, partial, stale, no-effect, syntax, and
  partial-effect behavior.

## B113-03 — Evidence controller and structural invariants

- Add a controller state separate from model message history: observation
  ledger, mutation epoch, named-check coordinates, attempts, failure digest,
  repair barrier, changed paths, and pause facts.
- Require complete fresh prior-turn evidence for existing-content mutation.
- Prevent a read and mutation in the same native multi-call model turn from
  satisfying the boundary.
- Advance one epoch only when a call produces at least one real byte/path
  effect; reject identity/no-effect actions.
- After a failed named check, require later-turn inspection before repair.
- Block the same named check at the same epoch before spawning its process.
- Normalize diagnostics and record stable SHA-256 failure fingerprints.
- Project these rules in `TraceStructure` so trace verification proves causal
  ordering rather than trusting summary counters.
- Gate: pure controller and malformed-trace matrices pass.

## B113-04 — Durable replay and recovery

- Write a versioned controller checkpoint alongside core recovery checkpoints.
- On resume, restore controller truth but conservatively stale inherited file
  evidence; reject missing/unsupported evidence checkpoints.
- Inject a machine-derived recovery packet containing pause reason, epoch,
  checks, last failure, changed paths, and reread requirements.
- Preserve the packet's literal rendered message for byte-stable replay.
- Keep clarification-answer ordering distinct from generic recovery and make
  resume-of-resume preserve both checkpoint streams.
- Prove compaction cannot alter controller facts.
- Gate: replay, pause/resume, clarification, compaction, and crash-prefix tests
  pass for legacy and evidence traces.

## B113-05 — Controlled live dispatch

- Order dispatch as guard, typed prepare, controller admission, human edit
  approval, sink approval, hash revalidation, commit, measured effect.
- Never invoke approval callbacks for controller-rejected actions and invoke
  each applicable approval exactly once for admitted actions.
- Emit typed call evidence and checkpoints in canonical trace order.
- Add concise, versioned general evidence-policy guidance to evidence system
  prompts only: inspect before edit, paginate incomplete reads, inspect after a
  failed check, and do not rerun unchanged checks.
- Preserve legacy behavior and direct-human chat passthrough.
- Gate: loop tests demonstrate read → edit → fail → inspect → repair → pass,
  plus every rejection boundary.

## B113-06 — Product-surface plumbing

- Thread optional harness policy through query, API, MCP, chat, ICM, common
  run configuration, and tests.
- Fresh omission remains the deliberate compatibility default; resume omission
  inherits the trace and explicit mismatch fails before workspace mutation.
- Render every new event without exposing hidden candidate bytes.
- Keep `trace verify` side-effect-free and based on shared structure logic.
- Gate: CLI/mock integration and literal compatibility fixtures pass.

## B113-07 — Reproducible server and paired autonomy runner

- Add optional llama-only seed/parallel fields to server argv and additive
  runfile metadata; do not pretend unsupported engines provide them.
- Record model, engine, context, sampling, listener, child-binary, corpus, and
  harness-policy provenance distinctly.
- Add frozen legacy/candidate arms; copy and hash each executable before a run,
  reject identical/missing binaries, and build shared child argv so only the
  executable and candidate policy flag differ.
- Interleave adjacent counterbalanced AB/BA arms on fresh workspaces.
- Extend result coordinates, retained-trace names, completeness, mechanism
  metrics, per-policy aggregation, and infrastructure-clean paired scoring.
- Keep pass-power and repository-brief comparisons partitioned by policy.
- Gate: pure scheduling/result tests plus CLI process-boundary tests pass.

## B113-08 — Evidence-only decision and planner boundary

- Build and hash a candidate only after all Rust gates pass.
- Run the frozen real-model screen and permit at most two retained,
  trace-justified general revisions if it remains 0/3.
- Freeze the candidate before paired confirmation and held-task evaluation.
- Do not implement `EvidencePlanner` until evidence-only clears its screen and
  a separate addendum chooses an embedded phase or typed linked traces.
- Never silently fall back from a labeled planner arm to evidence-only.

## Commit discipline

Each build unit is independently formatted, compiled, tested, synced from the
active task ledger to completed tasks, and committed on `dev`. No sprint branch
or worktree is created. The final PR is `dev` to `main`; only the owner merges.
