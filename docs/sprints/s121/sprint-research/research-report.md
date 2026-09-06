# Sprint 121 Research Report

## Intents Reviewed

- [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) — selected and clarified; active. Advance the explicit timeout/output-control and attributable-evidence portions of AC-11/12, not automatic speed calibration, reasoning, or application acceptance.
- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — selected; active. Preserve AC-6/11/12 source-owned execution, expert compatibility, and the small human decision surface while enabling later workflow integration.

## 1. Sprint Goal

Deliver the bounded T-11505 budget-control prerequisite to the larger-model
application trial: an optional context-reserve-bounded action output cap for
expert query/benchmark use and a positive finite benchmark execution-time
scale, with exact request/result/trace attribution and no silent durable
capability promotion. Keep omitted defaults, ordinary `cargo r`, tool authority,
and checked process cleanup unchanged. This is infrastructure for later
measurement, not another mandatory first-run preflight or a completed model
application experiment.

Owner-merged PR 108 is the baseline: `ffab58de35c7dd341ae35f43bc06fb5794b52c59`.
`dev` was fast-forwarded to that commit; no open dependency intake existed.
The installed 0.22.0 substrate check passed and initialized Sprint 121.
Research began at 13:07:50 UTC on 2026-09-05. All findings below are source
inspection, not newly executed model or test results. Two bounded independent
surveys and a separate scope review were reconciled by the primary agent.
Sprint 120 has no `failure-report.md`; its retained checkpoint failures,
renewed acceptance and scheduling caveat were read in its Loop record.

## 2. Existing Code Survey

| File | Relevance | Notes |
|------|-----------|-------|
| `crates/ferric-cli/src/main.rs` | medium | Normal front door and expert `bench full`/query dispatch remain distinct. |
| `crates/ferric-cli/src/query.rs` | high | Policy is selected before sampler; no output override; fresh per-segment budgets and printed resume hint need explicit compatibility. |
| `crates/ferric-cli/src/bench_cmd.rs` | high | No scale/output control; per-run rows, summary, single/fleet calibration and retained traces are composed here. |
| `crates/ferric-cli/src/human.rs` | medium | Human Ask/Work preparation must not gain prompts or mandatory benchmarking. |
| `crates/ferric-core/src/scale.rs` | high | Output caps 512/768/1024/1536/2048/2048; prompt reserve alone is context-adjusted. |
| `crates/ferric-loop/src/run.rs` | high | Actual sampler and nominal policy cap can diverge for direct callers; truncated constrained actions are not dispatched. |
| `crates/ferric-loop/src/compact.rs` | high | Summarization uses independent default sampling; action override must not retune it. |
| `crates/ferric-loop/src/replay.rs` | medium | Existing resume projection does not inherit an action override; do not silently invent whole-policy inheritance. |
| `crates/ferric-provider/src/openai.rs` | high | Both response modes use literal `max_tokens`; usage conversions contain unchecked u64-to-u32 casts. |
| `crates/ferric-provider/src/types.rs` | high | Fixed-width token field, request validation and `truncated` contract; token count is not a wall-clock deadline. |
| `crates/ferric-provider/src/lib.rs` | medium | Provider changes require actual real-model evidence under the repository's stated contract. |
| `crates/ferric-trace/src/event.rs` | high | `PolicySelected` records output cap; additive trace vocabulary and legacy reading are available. |
| `crates/ferric-trace/src/lib.rs` | medium | Existing policy/legacy-event test locations identified; no broad trace rewrite proposed. |
| `crates/ferric-bench/src/runner.rs` | high | Parent owns execution deadline; fresh and continuation argv builders are separate. Mock child currently does not receive context/parameter flags. |
| `crates/ferric-bench/src/process.rs` | medium | Re-exports the shared source-owned process runner; no separate timeout implementation. |
| `crates/ferric-bench/src/spec.rs` | high | Agent timeout and trusted post-run check timeouts are distinct contracts. |
| `crates/ferric-bench/src/results.rs` | high | Append-only rows lack requested/effective budget metadata; old rows must remain readable. |
| `crates/ferric-bench/src/calibrate.rs` | high | Evidence eligibility gates publication; profile writer is non-atomic and defaults corrupt input to empty. |
| `crates/ferric-bench/src/provenance.rs` | medium | Existing SHA-256 helpers can bind a parent budget record to unchanged retained trace bytes. |
| `crates/ferric-bench/src/summary.rs` | high | Timeouts already have a separate counter, but full diagnostic sweeps could still produce eligible calibration evidence. |
| `crates/ferric-process/src/lib.rs` | high | Execution uses elapsed-duration comparison and checked cleanup; no new lifecycle implementation is needed. |
| `crates/ferric-cli/tests/bench_mock.rs` | high | Source-owned whole-harness regression lane, retained traces/results, single/fleet/profile cases. |
| `crates/ferric-cli/tests/cli.rs` | high | Query defaults, malformed numbers, external traces and real-shell resume-argv regression seams. |
| `.github/workflows/ci.yml` | high | Canonical controlled native schedule and default/backend-free/platform gates remain unchanged. |
| `crates/ferric-bench/specs/l0.toml` | medium | Agent timeout 60 s; keep spec unchanged. |
| `crates/ferric-bench/specs/l1.toml` | medium | Agent timeout 90 s; keep spec unchanged. |
| `crates/ferric-bench/specs/l2.toml` | medium | Agent timeout 180 s; keep spec unchanged. |
| `crates/ferric-bench/specs/l3.toml` | medium | Agent timeout 180 s, separate grader bounds. |
| `crates/ferric-bench/specs/l4.toml` | medium | Agent timeout 300 s, separate grader bounds. |
| `crates/ferric-bench/specs/l5.toml` | medium | Agent timeout 600 s, separate grader bounds. |
| `crates/ferric-bench/specs/l6.toml` | medium | Agent timeout 900 s, separate grader bounds. |

Authority/context read separately: project `AGENTS.md`, both reviewed intent
chapters, work/confidence ledgers, Sprint 120 metadata, its
[Loop reconciliation](../../s120/loop-review.md) and selective
[repository review](../../s120/sprint-research/repository-review.md).

### Findings

- **R01 — missing explicit controls.** `query.rs:486-495` copies the tier cap
  into sampling. `bench_cmd.rs` exposes neither requested control and
  `runner.rs:152-155` always uses the embedded agent timeout.
- **R02 — context semantics must be narrow.** At declared context 4096, the
  Large tier's 2867 prompt budget plus 1536 output cap exceeds context. An
  explicit-only checked rule `0 < N <= ctx - prompt_budget_tokens` preserves
  historical defaults without pretending that the heuristic prompt budget
  tokenizes/admissibly bounds every actual request or measures hardware fit.
- **R03 — cap and provenance must agree.** The loop sends sampling at
  `run.rs:284-286`, but `PolicySelected` records policy at line 1050. The new
  supported CLI route must resolve them together; actual request sampling
  must be inspectable without mislabelling nominal policy as wire evidence.
- **R04 — truncation is a safety boundary.** `run.rs:384-406` dispatches no
  constrained action on a truncated response, retries once at the same cap,
  then stops. Larger output must not bypass that boundary or enlarge rings.
- **R05 — compaction is separate.** `compact.rs:134-146` uses its own default
  sampler. The new cap concerns main action completions only; no reasoning or
  compaction-profile claim follows.
- **R06 — preserve execution scope.** Scaling concerns the parent-controlled
  agent segment only, not grading, startup, native fixture or cleanup limits.
  Reject invalid factors, overflowing products and positive values rounding
  to zero before any workspace/result creation or child launch.
- **R07 — evidence must survive an early timeout.** A child may die before
  completing or creating a trace. Parent budget/termination records cannot
  depend on a successful terminal child event. Do not append fabricated
  completion events to the child's trace.
- **R08 — diagnostic-only must reach the summary gate.** Avoiding
  `write_profile` is insufficient: `summary.rs` can still advertise eligible
  calibration and `calibrate_from_evidence` consumes it. A non-default scale
  or any explicit output override must retain observations but mark persisted
  calibration ineligible, with a reason, in single and fleet flows. Existing
  profile bytes, including corrupt bytes, remain untouched on this lane.
- **R09 — mock child mismatch.** Real child argv receives context/parameter
  values; mock argv does not. New budget validation and evidence must use the
  context actually passed to both routes. Preserve frozen legacy continuation
  argv when no new control is requested.
- **R10 — endpoint/fingerprint limits remain.** Per-trial implicit discovery,
  unbound profiles and atomic persistence remain T-12023/T-11507. A pinned
  explicit endpoint in acceptance prevents rediscovery for that exercise;
  it does not establish immutable runtime/model identity by itself.
- **R11 — overflow follow-up.** `openai.rs:339-340,579-582` narrows usage counts
  with `as u32`, so an oversized server count can wrap. This is a separately
  retained source-identified gap, not part of new CLI budget validation.
- **R12 — human wording.** Existing debug/Candle speed advice in benchmark
  code is stale for external inference. Correct that touched advice; do not
  expand the primary help or ask humans to set calibration controls.

## 3. External Sources

- [Rust Duration](https://doc.rust-lang.org/std/time/struct.Duration.html#method.try_from_secs_f64) — checked conversion rejects nonfinite/negative/overflow values, but tiny positive values can round to zero; explicit zero rejection is still needed. Accessed 2026-09-05.
- [Tokio timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html) — cancellation drops the future, but non-yielding work is not made preemptible. Token caps and async deadlines do not establish whole-process lifetime guarantees. Accessed 2026-09-05.
- [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) — current chat-completion transport supports streaming and schema constraints; reasoning controls are adapter features, not something to infer from a model name. Current upstream documentation is design context, not qualification of the installed runtime. Accessed 2026-09-05.

## 4. Risks, Unknowns, Dependencies

- **Risk:** a large output allowance is an explicit resource commitment, not
  permission to run more tools. Preserve ring/controller/guard/approval state.
- **Risk:** silently clipping an excessive request or shrinking the prompt
  changes the experiment. Reject invalid explicit values with one safe action.
- **Risk:** changing omitted defaults would invalidate comparisons. Pin tier
  defaults, scale 1.0, old result deserialization and frozen continuation argv.
- **Risk:** timeout may coexist with missing trace or failed checks. Retain
  each observation and classify execution budget exhaustion separately; do not
  infer that the model itself lacks capability or rewrite an observed failure.
- **Risk:** new budgeting can be conflated with profile qualification. The
  durable intent now makes diagnostic ineligibility explicit until coordinate-
  bound calibration is implemented.
- **Dependency:** T-12023/T-11507 own profile coordination, fingerprints and
  runtime discovery; T-11508 owns reasoning/compaction; T-11506/T-11410/T-11412
  still own the frozen live app sequence. None is completed here.
- **Dependency:** native child tests use source-aware Cargo execution and
  checked reaping. Keep Sprint 120's schedule and its unresolved T-12027 cause
  caveat. No retry-to-green, manual cleanup repair or direct target execution.
- **Deferred:** T-12020's high-priority ICM ingestion issue remains open; the
  human path grants no ICM. T-12024 whole-Work/Git cancellation and T-12028
  Cargo target warning are not silently absorbed into this budget increment.
- **Unknown:** no new local model speed or successful large-model application
  result has been measured. A bounded live request proves only its exercised
  transport/budget/cleanup behavior; deterministic large-action tests carry
  the exact byte-level and truncation assertions.

## 5. Recommended Approach

1. Add one optional expert spelling, `--max-output-tokens`, to query and
   `bench full`. Resolve an explicit cap once against the declared effective
   context reserve, apply it to policy and main-action sampling, and retain
   requested/effective/source data. No option means historical behavior.
   Overrides remain invocation-scoped; printed query resume guidance repeats
   an explicit cap, while a manually omitted cap retains existing fresh-
   segment semantics. Do not introduce implicit whole-policy inheritance.
2. Add `bench full --timeout-scale` with default 1.0. Calculate checked
   effective agent duration once per selected spec before effects, and carry
   it to the existing owned runner. Record base/scale/effective duration,
   requested/observed output caps, declared context, warmup `not_performed`,
   and parent termination cause. Bind a parent-authored budget sidecar to
   retained trace bytes by SHA-256; preserve trace absence explicitly and
   never rewrite child evidence. Rows and summary expose the same controls.
3. Make modified-budget sweeps evidence-only automatically: no extra human
   switch, no durable measured-level claim, and no profile mutation. Preserve
   default calibration behavior and raw per-task results. This narrow gate
   does not claim to repair the existing general-purpose profile writer.
4. Prove default, invalid/overflow, real/mock argv, large valid JSON action,
   truncated zero-dispatch, resume guidance, persisted provenance and
   single/fleet profile preservation. Run fresh native CI and a bounded
   source-owned existing-model exercise after implementation. Correct touched
   expert documentation, leaving the README/front door concise.

Alternative considered: immediately automate acquisition/calibration or run
the frozen app with hand-adjusted source constants. Rejected for this increment:
it would combine unresolved attribution/persistence dependencies and obscure
the exact controls under test. Another alternative, merely increasing global
defaults or all timeouts, would invalidate baselines and weaken unrelated
bounds. Full endpoint/profile redesign is useful but independently tracked;
diagnostic budgets do not need to claim that redesign complete.

## Artifacts

- This report consolidates the two read-only source surveys and independent
  scope review; no raw runtime result was generated during Research.
- [Sprint metadata](../sprint-meta.md) records the merge boundary, protected
  edit and phase handoff.
- [Stable work ledger](../../../work/tasks.md#post-sprint-115--ordered-local-model-work)
  retains T-11505 and the prerequisite order; the new usage-count gap is
  retained separately in the Sprint 121 research follow-up section.

## Budget Override

The 31-file source/config survey exceeds the 20-file cap because the requested
budget value crosses CLI admission, policy, main generation, independent
compaction, transport, trace/replay, source-owned benchmark execution and
calibration publication. Seven rows are unchanged embedded spec timeout
lookups, not seven new implementation areas. The survey intentionally includes
mock/fleet/continuation and CI consumers to avoid false provenance or weakened
cleanup. Three external sources stay below the five-source cap; research is
bounded to 30 minutes, with no model load, full-suite preflight or new lifecycle
investigation.

Research ended at 13:27:17 UTC (19 minutes 27 seconds after initialization).
The installed helper reported `files=31 sources=3` and exit 1 for its numeric
file cap. This explicit cross-cutting Budget Override is the phase contract's
alternative to the numeric cap; the helper was not falsely reported passing.
