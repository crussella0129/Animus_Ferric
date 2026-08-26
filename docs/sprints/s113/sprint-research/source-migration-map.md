# Sprint 113 Source Migration Map

This is a pre-approval implementation map, not an approved build or
test plan. It records the exact seams inspected at baseline commit
`cabe2368154339013c39958da43580db86e19f78` so the approved build can proceed in
compile-safe, independently testable commits.

## 1. Core policy identity

### Files

- new `crates/ferric-core/src/harness.rs`
- `crates/ferric-core/src/lib.rs`

### Types

Add a serde/clap-independent core enum:

```rust
enum HarnessPolicy {
    Legacy,
    Evidence,
    EvidencePlanner,
}
```

`Legacy` is the serde default for old traces and result rows. The command-line
wrappers may define their own `ValueEnum` conversion so `ferric-core` remains
independent of clap.

This identity is orthogonal to:

- model-derived `RunPolicy`
- `ActionProtocol`
- autonomy `variant` (`current`, `recovery`, `repository_brief`)
- tool permission and sink policies

Fresh-run default selection is a product decision made after the experiment.
Continuation defaults inherit the recorded policy; an explicit mismatch fails
closed.

## 2. Trace wire additions

### Files

- `crates/ferric-trace/src/event.rs`
- `crates/ferric-trace/src/lib.rs`

Keep `TRACE_SCHEMA_VERSION = 1` because the additions are new event variants and
defaulted fields. Keep `RECOVERY_CHECKPOINT_VERSION = 1`; do not smuggle
safety-critical state into the old checkpoint under optional defaults.

Add defaulted `harness_policy` to `Event::PolicySelected`, plus versioned wire
types and events for:

- `ObservationRecorded`
- `ControllerBlocked`
- `WorkspaceEffectRecorded`
- `VerificationCheckRecorded` with pass/fail outcome
- `ControllerCheckpoint`
- `RecoveryPacketInjected`

Keep legacy `WorkspaceMutation` and `VerificationCheckPassed` readable and valid
under `legacy`. Evidence sessions use the new effect/check events so their
stronger semantics do not retroactively reinterpret old traces.

Wire observations include file/search/find variants. File observations carry:

- normalized workspace-relative path
- full-content SHA-256
- total bytes and logical lines
- requested and returned inclusive ranges
- whether the complete returned slice reached the model
- whether the slice covers the complete file
- whether registry truncation occurred

Path effects carry path, kind, and optional before/after SHA-256. A call that
changes multiple paths advances the global mutation epoch once.

Literal backward-compatibility fixtures must prove that old policy events still
deserialize as `legacy` and old traces contain no implied controller state.

## 3. Typed tool-control boundary

### Files

- new `crates/ferric-tools/src/control.rs`
- `crates/ferric-tools/src/spec.rs`
- `crates/ferric-tools/src/registry.rs`
- `crates/ferric-tools/src/lib.rs`
- `crates/ferric-tools/Cargo.toml` (`sha2`)

Keep `Tool::run` and `Registry::execute` as the legacy/human path. Add an
evidence path rather than forcing human `chat !run` and all old registry tests
through model-controller behavior.

Add runtime-only classifications and preparation data:

```text
ControlKind
  FileObservation
  WorkspaceNavigation
  ContentMutation
  StructuralMutation
  NamedCheck
  MetadataMutation
  ReadOnly
  OpaqueMutation

PreparedControl
  Observation(snapshot + render data)
  ContentMutation(exact before/candidate bytes + syntax states)
  StructuralMutation(declared target snapshot)
  NamedCheck(name)
  Direct

ExecutionEffect
  Observed
  Changed(path deltas)
  Unchanged
  CheckPassed
  CheckFailed(full diagnostic)
  None
```

The evidence dispatch order is:

1. registry boundary/ignore/command checks
2. side-effect-free preparation
3. deterministic controller admission
4. human accept-edits approval
5. sink-policy approval
6. immediate current-hash revalidation
7. exact prepared commit
8. typed effect measurement, independent of success/error

Unknown/custom `Write` or `Execute` implementations default to
`OpaqueMutation`. Evidence mode must fail closed on unmodeled content effects;
a name table in `run.rs` is not an acceptable bypassable substitute.

`ExecuteOutcome::Completed` gains typed effect metadata. Existing destructuring
already predominantly uses `..`; direct human execution receives `None` and
keeps current behavior.

## 4. Built-in tool preparation

### Navigation

- `crates/ferric-tools/src/builtin/read_file.rs`
- `find_files.rs`
- `search_files.rs`

`read_file` prepares an immutable content snapshot and the registry renders the
observation envelope after it knows the model truncation limit. A truncated
model view establishes no range coverage. Partial non-truncated reads may be
merged only while the full-file digest remains unchanged.

`find_files` and `search_files` return explicit zero-match messages naming the
literal pattern/query and normalized root. Their typed observations also record
match count, cap, exhaustion, and a result digest.

### Content mutation

- `write_file.rs`
- `edit_file.rs`
- `multi_edit.rs`
- `apply_patch.rs`
- optional shared `content_mutation.rs`

All four build exact candidate bytes without committing. Reject:

- empty mutation lists/patches as today
- `old_string == new_string`
- a full candidate identical to current bytes
- a multi-edit/patch whose net result is identical

Preparation returns current/candidate bytes, normalized path, before/after
digests, and success text. Commit writes the exact prepared candidate only after
the current digest is revalidated.

### Structural mutation

- `copy_file.rs`
- `move_path.rs`
- `delete_path.rs`
- `make_dir.rs`
- `git_write.rs`

Ring-zero structural tools cannot become an evidence-policy escape hatch.
Declare sources/destinations and measure actual path effects. Existing
destination content requires evidence; recursive directory deletion/move must
either prove all affected files were observed or fail closed as unmodeled.
`git add`/`commit`/`branch` are metadata-only; `git checkout`, which can rewrite
the worktree, is blocked until modeled. `.git` and `.ferric` changes never
manufacture a source mutation epoch.

## 5. Candidate syntax admission

### Files

- `crates/ferric-tools/src/builtin/check_syntax.rs`
- the four content tools above

Replace post-write `py_compile` warning behavior with an in-memory status API.
For Python, pass candidate UTF-8 through stdin to `ast.parse`; do not create a
temporary committed file or embed paths/content into shell text.

Return `Valid`, `Invalid(diagnostic)`, or `NotChecked(reason)`. Enforce:

- valid → invalid: block atomically
- invalid → valid: allow
- invalid → changed invalid: allow with explicit warning
- absent → invalid: block atomically
- unsupported extension or missing interpreter: allow but record `NotChecked`

Tests inject or detect validator availability rather than silently passing a
broken-Python assertion when Python is missing.

## 6. Evidence and repair controller

### Files

- new `crates/ferric-loop/src/controller.rs`
- `crates/ferric-loop/src/lib.rs`
- `crates/ferric-loop/Cargo.toml` (`sha2`)

`ControllerState` owns the file evidence ledger, check attempts, last failure,
changed paths, and repair barrier. It remains separate from model-generated
messages; compaction can therefore change prompt history without changing
controller truth.

Admission invariants:

- existing content requires complete current evidence
- evidence must come from an earlier model turn
- current hash must equal the observed hash
- new files may be created without file evidence
- a successful authored mutation records its exact postimage as evidence, but
  it cannot authorize another call proposed in the same batch
- after a failed check, repair targets require a later-turn fresh observation
- a same-name check at the same mutation epoch is blocked before process spawn
- no-effect and blocked actions never advance the epoch
- one or more actual path effects advance it exactly once

Normalize full check diagnostics (CRLF to LF, canonical workspace prefix to
`<workspace>`, trailing whitespace trimmed) before SHA-256. Preserve the full
diagnostic in `ToolResult`; the fingerprint is controller identity, not a lossy
replacement for feedback.

Controller errors give a compact next action so the small model does not spend
turns rediscovering the policy.

## 7. Loop integration and approval ordering

### Files

- `crates/ferric-loop/src/run.rs`
- `crates/ferric-loop/src/projector.rs`
- `crates/ferric-loop/tests/accept_edits.rs`
- `verification_gate_tests.rs`
- new focused controller integration tests

Add `harness_policy` to `RunArgs` and `LoopState`. Under `legacy`, preserve the
existing dispatch path. Under `evidence`, use prepare → admit → approve → commit.

This specifically moves accept-edits preview after machine admission. A human
must not be asked to approve an action the deterministic controller will reject.

Emit typed evidence events between the matching `ToolCall` and `ToolResult`.
Controller blocks still produce an errored `ToolResult`, count toward existing
bounded failure guards, and do not count as actual tool/check executions.

Append concise general evidence-policy guidance to the system prompt on fresh
evidence sessions. It teaches read-before-edit, pagination, inspect-after-check,
and no unchanged rechecks. The literal `SessionPrompt.system` remains the
durable source; no task-specific facts are introduced.

## 8. Structural validation, projection, and replay

### Files

- `crates/ferric-loop/src/trace_structure.rs`
- `projector.rs`
- `replay.rs`
- `recovery_protocol_tests.rs`
- `resume_tests.rs`
- `clarification_tests.rs`
- `compaction_tests.rs`

Extend `TraceStructure`; do not create a second controller validator in
`trace_verify.rs`. Explicitly route every new known event so its wildcard cannot
silently accept impossible evidence.

Validate call IDs, tool/check identity, outcome/error agreement, nonempty real
effects, epoch increments, observation freshness, later-turn repair inspection,
unique check execution per `(name, epoch)`, controller checkpoint pairing, and
recovery-packet facts.

Fresh evidence sequence:

```text
SessionStart → PolicySelected(evidence) → [PromptComposed] → SessionPrompt
→ ControllerCheckpoint(initial) → TurnStart
```

Resume sequence:

```text
SessionStart(resumed_from) → PolicySelected(same) → RecoveryCheckpoint
→ ControllerCheckpoint(stale observations) → [ResumePrompt + both anchors]
→ RecoveryPacketInjected → TurnStart
```

Clarification-answer continuation omits the generic pause packet. A generic
goal amendment keeps it after the durable answer/amendment anchors.

Pause sequence:

```text
SessionEnd(non-success) → RecoveryCheckpoint → ControllerCheckpoint
→ SessionPaused(same reason)
```

`ReplayedState` carries recorded harness policy and controller state.
`validate_resume_target` rejects explicit policy changes. Old traces remain
legacy-resumable; an old binary encounters unknown controller events and fails
closed when asked to replay a new evidence trace.

## 9. CLI surface plumbing

### Files

- `crates/ferric-cli/src/query.rs`
- `api.rs`
- `mcp.rs`
- `chat.rs`
- `icm.rs`
- `config.rs`
- `crates/ferric-cli/tests/cli.rs`

Thread policy through `RunConfigArgs`, `RunConfig`, `LoopSetup`, and every
`RunArgs` constructor. Current constructor inventory includes roughly 25 loop
test constructors, eight run-config constructors, and six loop-setup
constructors, so add/update common test builders before leaf fixtures.

Use an optional CLI/config override. On resume, omission inherits the trace;
explicit mismatch is rejected. This prevents a future evidence default from
making old interrupted sessions unexpectedly unresumable.

Update `trace cat` rendering for every new event. Keep `trace verify`
side-effect-free and delegate structural/controller truth to `TraceStructure`;
its CLI-local state remains for envelopes and summary reporting only.

## 10. Managed-server reproducibility metadata

### Files

- `crates/ferric-cli/src/server.rs`
- `crates/ferric-cli/src/backend.rs`

Expose optional llama-only `--seed` and `--parallel` controls, add them to
`ServerConfig`/argv, and store additive values in `ServerRunfile`. Ollama must
not silently claim unsupported values.

The frozen autonomy runner already passes `--temperature 0`; llama.cpp defines
temperature zero as greedy selection, so the original server default seed did
not make the control stochastically sampled. Seed/slot capture is
reproducibility hardening and future-proofs nonzero-temperature work, not a
reason to invalidate the original result.

For confirmation, use one slot, temperature zero, and recorded engine build,
model hash, context, seed, and argv. Three repetitions at the same deterministic
settings measure reproducibility/stability; do not describe them as independent
Bernoulli samples.

## 11. Autonomy arm and provenance migration

### Files

- `crates/ferric-bench/src/runner.rs`
- `summary.rs`
- `autonomy_results.rs`
- `lib.rs`
- `crates/ferric-cli/src/autonomy_cmd.rs`

`Invocation` gains an optional child harness-policy flag. The frozen control
invocation omits the unknown flag and uses the preserved binary; the candidate
invocation passes `--harness-policy evidence`.

Add paired command arguments for control/candidate binaries. Canonicalize, hash,
and copy both into run-owned evidence before the matrix. Separate orchestrator
binary provenance from child agent binary provenance; the SHA-256 is the
authority when the old build lacks an embedded commit.

The schedule is:

```text
trial → task → autonomy variant → counterbalanced AB/BA harness policies
```

Each arm gets a fresh task workspace/profile directory. Record arm position and
server state. Retained trace names include harness policy to prevent collision.

Add defaulted row fields/counters and bump the autonomy result schema. Expected
coordinates become `(harness_policy, task, variant, trial)`. Add:

- per-policy and per-policy/variant summaries
- expected/observed/scoreable pairs
- legacy-only, evidence-only, both, neither
- paired objective and contract deltas
- infrastructure-unpaired count

Never score an infrastructure-dirty partner as a model loss. Keep pass-power
and repository-brief grouping separated by harness policy. A mixed-binary run
stores per-policy provenance rather than pretending it has one global binary.

Add observation/block/check/effect/recovery counters in
`AutonomyTraceMetrics`. Assert row policy matches `PolicySelected` and that all
new evidence traces pass `TraceStructure` before grading.

## 12. Planner arm boundary

The existing `ActionProtocol::Plan` terminates on `submit_plan` and each query
segment currently requires exactly one newly created trace. Do not build the
planner by merely toggling `RunPolicy.uses_planner`.

After evidence-only screening, freeze a design addendum choosing either:

- linked planner/executor traces with explicit roles and one executor
  continuation trace; or
- one trace with a checkpointed phase transition where `submit_plan` changes
  phase instead of ending the session.

If linked traces are selected, change runner discovery to return typed trace
artifacts, grade/continue only the executor, retain both, and aggregate metrics
by role. Until that design is frozen, `evidence_planner` is unavailable rather
than silently falling back.

## Compile-safe build units after approval

1. Core policy plus additive trace/result wire types and literal compatibility
   fixtures; no evidence writer yet.
2. Typed tool preparation/effect API, navigation envelopes, content candidates,
   structural classification, syntax admission, and tool-level tests.
3. Pure controller state/admission tests plus `TraceStructure` controller-event
   validation.
4. Projector/replay/checkpoint/recovery packet integration and resume tests.
5. Live loop controlled dispatch, prompt guidance, approval ordering, real
   effects, failed checks, and loop tests.
6. Query/API/MCP/chat/ICM policy plumbing and CLI compatibility tests.
7. Managed-server metadata, autonomy paired arms, metrics/provenance, and pure
   aggregation/scheduling tests.
8. Full Rust gates, real evidence screen, bounded general revisions, frozen
   candidate paired confirmation, and untouched held-task comparison.

Each build unit must format, compile, and pass its affected tests before its
atomic commit. No planner implementation enters units 1–7.
