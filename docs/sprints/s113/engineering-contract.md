# Sprint 113 Engineering Contract — Evidence-Bound Autonomous Recovery

## Execution contract

This is a repository-local engineering contract written in ordinary Markdown.
It is not an IDE artifact and has no approval callback, comment queue, sync
step, or hidden execution semantics. Implementation uses normal source edits,
Rust tooling, Git commits on `dev`, and recorded runtime evidence.

## Outcome

Turn the reproducible 0/3 long-horizon control into a causal harness experiment:
Ferric should force repository actions to be grounded in observed bytes and
verification feedback, then separately test whether its dormant read-only
planner adds further value. The model, task prompts, constrained protocol,
context, temperature, graders, and managed-server topology stay fixed.

## Engineering basis

The work is derived from the frozen Qwen control traces, Ferric's current tool
and replay architecture, and primary agent-interface research. Implementation
is divided into compile-safe work packages. Each package has explicit source
boundaries and evidence gates; passing unit tests alone cannot substitute for
the final real-server comparison.

## Approved engineering work packages

### 1. Evidence-bearing navigation

- Make `find_files`/`search_files` zero-result output explicit and retain their
  literal-query semantics in the observation.
- Envelope `read_file` results with canonical workspace-relative path, returned
  line range, total size, truncation state, and a deterministic full-content
  fingerprint.
- Carry typed observation/effect metadata through the tool registry; do not
  infer controller decisions from tool names or human-readable output.
- Add model-free tests for empty, partial, full, and truncated observations.

### 2. Observation-gated, effect-aware mutation

- Track successfully observed existing files in the loop.
- Reject content-sensitive edits/overwrites of an existing file until its
  current content has been observed; allow genuinely new-file creation.
- Do not let a read authorize a mutation proposed in the same native
  multi-call turn, and recheck the observed hash immediately before commit.
- Conservatively require rereading after resume when evidence freshness cannot
  be proven.
- Reject empty/identity/no-effect mutations and advance mutation epochs only
  when workspace bytes actually change.
- Prepare candidate bytes before committing them. Block valid-to-invalid
  syntax regressions and invalid new files; permit changed invalid-to-invalid
  candidates with an explicit warning so existing broken files can be repaired
  incrementally.

### 3. Verification-guided repair state

- Record failed named-check attempts with check name, mutation epoch, attempt,
  and diagnostic fingerprint.
- After a failure, require fresh inspection before another content mutation.
- Refuse execution of the same named check at the same mutation epoch, returning
  a recovery instruction instead of spending another check process.
- Keep the repair budget bounded by existing policy/guards and preserve the
  named-check security boundary.

### 4. Durable recovery packet

- Surface the replayed pause reason to the model.
- Add machine-derived facts—current mutation epoch, required/passed checks,
  last failed check/fingerprint when available, and the need to re-observe
  existing files—to non-clarification continuations.
- Ensure compaction cannot discard or override controller state.
- Verify resume, resume-of-resume, clarification ordering, and conservative
  evidence invalidation.
- Add concise general policy guidance to evidence-session system prompts so the
  model knows read-before-edit, pagination, repair inspection, and unchanged
  check rules before it wastes a turn. Do not add task-specific hints.

### 5. Separately measurable planner arm

- First implement and screen the evidence-only policy. The existing read-only
  `ActionProtocol::Plan` is terminal and cannot simply transition into the
  current execution session, so freeze a linked-session or embedded-phase
  design addendum before building the planner arm.
- Require a bounded structured plan containing observed target files,
  invariants, ordered steps, and the final named check.
- Validate that existing target files in the plan were actually observed and
  never widen the policy's tool ring.
- Link planner and execution trace provenance explicitly; a measured planner
  failure must not silently fall back to evidence-only.
- Preserve a legacy/evidence-only arm so planner value is measured rather than
  assumed. Do not promote the planner if it adds latency or anchoring without
  additional objective completions.

### 6. Metrics and provenance

- Extend trace/autonomy metrics for observations, blind-mutation blocks,
  no-effect blocks, syntax rejections, failed-check fingerprints, unchanged
  check blocks, repair attempts, recovery packets, and planner transitions.
- Add an orthogonal harness-policy identifier to results/provenance without
  repurposing the frozen task `variant` field.
- Extend the central trace state machine and add a separate versioned
  controller checkpoint for evidence sessions instead of duplicating
  verification logic in the CLI or weakening legacy checkpoint compatibility.
- Keep older traces and result rows readable.

## Verification ladder

1. Unit tests for each tool observation/mutation contract and diagnostic
   fingerprint.
2. Loop tests for read-before-mutate, real mutation epochs, check-failure → read
   → repair → pass, unchanged-check refusal, compaction, and recovery.
3. CLI/mock integration tests for policy selection, trace ordering, metrics,
   backward compatibility, and absence of arbitrary execution.
4. Rust gates: format, affected crates, workspace tests, and clippy with warnings
   denied.
5. Real-server evidence-only screening on frozen H01/H04/H08 with the exact
   model and topology used by the control.
6. Three-trial interleaved paired confirmation with the preserved control
   executable under identical temperature-zero, single-slot, fully recorded
   server settings. Temperature zero is greedy in llama.cpp, so repeats measure
   stability rather than independent stochastic trials; the original control
   remains diagnostic because it covers only three selected tasks.
7. Held-task comparison on H02/H03/H05/H06/H07.
8. Launch/status/query/down lifecycle validation and side-effect-free trace
   verification for every retained real-model trace.
9. Planner-arm implementation and comparison only after the evidence-only
   mechanism clears its screen and its orchestration design is frozen.

## Acceptance decision

- Promote evidence-bound repair only if it clears the research report's
  objective, safety, mechanism, and reproducibility gates.
- For the initial screen require at least 1/3 objective completions. For the
  three-trial paired confirmation require a positive paired completion delta
  and at least one task completed by evidence in at least 2/3 stability repeats.
- The three screening tasks are a development set. If the first screen stays at
  0/3, permit at most two retained, trace-justified revisions to general
  controller behavior; freeze the candidate before paired confirmation and do
  not inspect or tune against held tasks.
- Require zero admitted blind/stale mutations, zero admitted no-effect
  mutations, and zero repeated named-check process executions at the same
  mutation epoch. Intercepted policy blocks may be nonzero.
- Promote the planner only if the evidence+planner arm adds objective wins over
  evidence-only without safety or clarification regressions.
- If objective completion does not improve, retain the control and report the
  intervention as falsified; do not substitute efficiency metrics for success.

## Scope boundaries

- No task-specific prompt additions.
- No larger or different model.
- No offline/mock substitute for real-server validation.
- No arbitrary shell or widened tool ring.
- No full 72-coordinate population claim from the diagnostic matrix.
- No sprint branch; approved work lands directly on `dev` and the owner remains
  the only merger of the final `dev` → `main` PR.
