# Sprint 113 Research Report

## Intents Reviewed

- [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) — created during Book-v2 migration from the approved Sprint 113 engineering contract; relevance: this chapter is the stable authority for the evidence-controller outcome, boundaries, causal evaluation, and planner gate. Its terminal state is recorded in the intent chapter; this research section preserves the pre-build question and gates.

## Research question

Can Ferric's harness—not a task-specific prompt patch or a larger model—make the
pinned Qwen2.5-Coder-7B model materially better at multi-turn repository work
where later actions depend on facts learned from earlier files, tool results,
and verification failures?

The target is to determine whether improvement is possible and measurable. It
is not to assume the answer, claim exponential gains in advance, or substitute
single-turn tool syntax for long-horizon objective completion.

## Control identity — frozen before execution

- Base commit: `cabe2368154339013c39958da43580db86e19f78`
- Release binary SHA-256:
  `F6E636F80AD3AF22920C91A22AB0C5A1F0F4E8AFE56DFECEE77822061C8320F4`
- Autonomy corpus SHA-256:
  `BB0CE1EC3F12A917096690E5A286232BFA05394C3C3D22D0589CB25542446323`
- Model: `qwen2.5-coder-7b-instruct-q4_k_m.gguf`
- Model SHA-256:
  `509287F78CB4D4CF6B3843734733B914B2C158E43E22A7F4BF5E963800894D3C`
- Server: managed `llama-server`, loopback port 8080, context 8192, GPU
  layers 0; endpoint discovered through Ferric runfiles rather than passed to
  the benchmark.
- Protocol: grammar; temperature 0; no streaming; isolated profile and task
  workspaces; real provider only.

The control executable is copied into ignored Sprint 113 artifacts before any
candidate implementation can overwrite `target/release/ferric.exe`.

## Frozen control set

One process-cold invocation runs one trial of the `recovery` policy variant on
three long-horizon, two-file tasks:

| Task | Cross-step dependency |
|---|---|
| H01 — inventory reorder plan | understand validation in `models.py`, then use that contract from `reorder.py` while preserving input |
| H04 — deterministic build order | validate graph invariants in `graph.py`, then consume them in deterministic topological ordering |
| H08 — job queue state machine | construct job state in `job.py`, then preserve transitions, copies, FIFO order, and errors in `queue.py` |

This is the named **control test**. It is a diagnostic research control, not the
full 72-coordinate autonomy baseline and not a population accuracy estimate.
The preserved binary allows more control trials later without rebuilding old
code.

## Evidence captured per episode

- contract and objective result, authoritative grader output, terminal reason,
  wall time, turns, tokens, tool calls/errors, mutations, checks, completion
  gates, compactions, and unnecessary clarification;
- every process segment, expected/observed continuation, exit code, trace path,
  and trace SHA-256;
- binary/model/corpus identity and process-cold server state;
- side-effect-free `ferric trace verify` for every retained trace;
- independent server PID, command line, listener owner, `/health`,
  `/v1/models`, and clean teardown evidence.

## Comparison discipline for the build phase

The candidate must use the same model artifact, protocol, context, temperature,
tasks, graders, and server topology. Improvements will be evaluated in paired
coordinates, expanded to three trials with the preserved control binary, then
checked on held long-horizon tasks not used to choose the intervention.

A result is promising only if objective/contract completion improves without
more unnecessary questions, unsafe completion gates, infrastructure failures,
or hidden task-specific instructions. Lower turns or tool errors are secondary;
they cannot replace objective completion.

## Control results

Run `autonomy-1785678826061-46124-0` completed all 3 expected rows and
reported `infrastructure_clean: true`. The result was **0/3 contract passes**
and **0/3 objective completions**. The 95% Wilson interval for either rate is
approximately 0–56.15%; this deliberately small diagnostic control therefore
establishes a reproducible failure floor, not the model's population accuracy.

| Task | Terminal | Turns | Input / output tokens | Calls / errors / reported mutations | Check calls | Wall time |
|---|---:|---:|---:|---:|---:|---:|
| H01 | `oscillation` | 13 | 25,224 / 1,994 | 12 / 4 / 5 | 4 | 275.600 s |
| H04 | `needs_input` | 4 | 5,218 / 886 | 4 / 1 / 2 | 1 | 120.852 s |
| H08 | `repeated_failure` | 11 | 21,752 / 1,423 | 11 / 6 / 5 | 4 | 191.244 s |

One clarification was observed and it was unnecessary. No verification check
passed, no task reached a completion gate, and no continuation was expected by
these three task definitions despite the suite variant retaining the historical
name `recovery`.

Every retained trace passed side-effect-free verification:

- H01: 141 records, 13 turns, 12 calls, SHA-256
  `E44B1573B082FFD9DBEF43D39BE52DC64DCB30A39CD61E286C739DDCFC488181`;
- H04: 47 records, 4 turns, 4 calls, SHA-256
  `2416A11CD3ECA40DC699305C73C252341412690C67A9C5AD5D3A2EE08B791A2D`;
- H08: 124 records, 11 turns, 11 calls, SHA-256
  `38271661AEC3FAE0A2DFE8B1E28D0A22F6EBE3042B871A30050CCA1F5E862AB7`.

Before teardown, the managed server was independently confirmed as PID 43876,
`llama-server.exe`, with the frozen model in its command line, sole owner of
`127.0.0.1:8080`, healthy at `/health`, and serving the exact model at
`/v1/models`. `ferric server down` then removed the process and runfile; the
PID, listener, and matching model process were independently confirmed absent.

## Failure analysis

The three failures are different expressions of one controller gap: the model
can emit valid constrained tool syntax, but Ferric does not require its next
action to be grounded in the observation or failure it just received.

### H01 — destructive replacement followed by false repair progress

The model passed a space-joined pair of filenames to a literal-substring
`find_files` tool. Its empty output was represented as an empty successful
result, and the model claimed the files had been found. It subsequently read
both files, but replaced the indented `return []` in `reorder.py` with a second
complete `def build_reorder_plan(...)`, creating a nested function and an
`IndentationError`. Four check attempts returned the same diagnosis. The model
then repeatedly proposed byte-identical old/new replacements; `edit_file`
reported these no-effect edits as successful mutations until the oscillation
guard stopped the run. Its `models.py` validation also incorrectly rejected
zero-valued integer fields.

### H04 — blind overwrite and unnecessary dependency escalation

The model read neither existing file. It overwrote `graph.py` and
`build_order.py` with standalone example programs, introduced an undeclared
`networkx` dependency, returned a single ready task instead of the required
order, and printed at import time. When the fixed check reported that
`networkx` was unavailable, it asked the user to install the package instead
of inspecting and repairing the repository with the standard library. This is
the control's unnecessary clarification.

### H08 — blind overwrite and unchanged-diagnostic loop

The model again read neither existing file. It put `JobQueue` in `job.py`, used
`id` where the contract required `job_id`, exposed caller-owned dictionaries,
and added import-time example execution. The check precisely reported
`KeyError: 'job_id'`. Rather than inspect either file or repair that contract,
the model alternated the unchanged check with narrow `job['id']` substitutions.
The diagnostic remained byte-identical through four check attempts; two final
edits targeted text no longer present and the repeated-failure guard stopped
the run.

### Contrast with retained Sprint 112 recovery traces

The earlier R08 continuation chain preserved the workspace, messages, turn
index, mutation epoch, and resume links correctly, but failed after producing
the same nested-function edit pattern and repeatedly ignoring the resulting
`IndentationError`. Across the audited retained recovery episodes, resume links
were observed 4/4 and two of three episodes ultimately recovered. The primary
failure mechanism is therefore strategy and evidence use after transport, not
lost continuation state or HTTP instability.

## Internal code findings

- `RunPolicy.uses_planner`, `max_plan_steps`, and `max_turns_per_step` are
  calculated and snapshot-tested but not consumed by the runtime. A read-only
  `ActionProtocol::Plan` and `submit_plan` terminator already exist.
- `find_files` returns an ambiguous empty string for zero matches even though
  its pattern is literal. `read_file` returns content without a machine-readable
  path/range/fingerprint envelope.
- Existing files can be overwritten without first observing them. Nothing in
  the loop records which file bytes the model actually saw.
- `edit_file` accepts `old_string == new_string`; the loop then advances the
  mutation epoch because the handler returned success even though bytes did not
  change. The same risk exists for other content mutations that reproduce the
  current bytes.
- Python syntax checking currently happens only after `write_file` has already
  written invalid bytes. It is a warning, not an atomic admission check, and it
  is absent from `edit_file`, `multi_edit`, and `apply_patch`.
- A failed named check is only an ordinary tool error. There is no durable
  diagnostic fingerprint, no block on rerunning it at an unchanged mutation
  epoch, and no requirement to inspect affected state before another edit.
- Replay reconstructs `pause_reason`, but the resumed model never receives it.
  Machine facts about the last failure and changed paths are not pinned outside
  the model-generated compaction summary.
- The existing cycle and repeated-failure guards correctly cap waste. They do
  not create a recovery strategy; by the time they stop these runs, the useful
  budget has already been spent.

## External evidence

The external literature supports testing the harness as a causal factor, but
does not justify assuming an exponential gain:

- [SWE-agent's ACI paper](https://arxiv.org/abs/2405.15793) reports that an
  LM-centered repository interface materially changes coding-agent performance.
  Its documented interface rejects syntactically invalid edits, bounds file
  views, provides repository search, and makes empty output explicit. Those
  mechanisms closely match the observed Ferric failures; they are interface
  controls, not larger-model substitutions.
- [Agentless](https://arxiv.org/abs/2407.01489) reached strong SWE-bench Lite
  results with an explicit localization → repair → validation pipeline. Its
  relevant lesson here is that a small, inspectable phase controller can beat
  unconstrained action selection; its absolute benchmark score is not directly
  comparable to this local three-task control.
- [RepoCoder](https://aclanthology.org/2023.emnlp-main.151/) found iterative
  retrieval-generation stronger than both in-file completion and one-shot
  repository retrieval. That supports cycling through fresh repository evidence
  as the proposed plan or repair changes, rather than treating a filename-only
  brief as sufficient context.
- The official [llama.cpp sampling documentation](https://github.com/ggml-org/llama.cpp/blob/master/tools/completion/README.md#temperature)
  states that temperature zero always selects the most likely next token, while
  its [server documentation](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md#sampling-params)
  records `-1` as the random seed default. Ferric's frozen autonomy runner sends
  temperature zero, so the unset server seed did not make this control a
  stochastic sample. The three selected tasks still form only a diagnostic
  development baseline.

These sources make a harness effect plausible. Only the pinned paired runs can
establish its size for this 7B quantized model and Ferric's task distribution.

## Intervention decision

The first candidate will be an **evidence-bound repair controller**, not a
task-specific prompt and not a planner-only intervention:

1. Make navigation results self-describing. Zero-match filename search must say
   that it found zero matches and that the query is literal; file reads must
   identify the observed path/range and content fingerprint.
2. Require a successful observation of an existing file before a
   content-sensitive mutation. A new file remains creatable. A resumed run may
   conservatively require a reread rather than trust stale evidence.
3. Admit only real mutations. Reject identity replacements and unchanged full
   writes, compare before/after bytes, and apply supported syntax checks before
   committing invalid content. Return compact evidence about what changed.
4. After a failed named check, enter a bounded repair state: require fresh
   inspection before another mutation and refuse the same check at an unchanged
   mutation epoch. Preserve fixed named checks; do not add arbitrary shell.
5. Put the prior pause reason and machine-derived recovery facts into every
   non-clarification continuation. Model-generated compaction may summarize but
   cannot replace these facts.
6. Trace and summarize observed-before-mutation, blocked blind/no-effect
   mutations, failed-check fingerprints, unchanged reruns, repair attempts, and
   recovery packets.

The dormant read-only planner will be a **separate second arm** after those
factual-state foundations exist. It should produce a bounded structured plan
whose target files were actually observed. Keeping it separate lets the test
distinguish “better evidence and recovery” from “same-model planning helped.”

### Falsifiable promotion gates

- Screening: on H01/H04/H08 trial 1, the candidate must complete at least one
  objective and contract versus the frozen control's 0/3, with zero
  infrastructure errors, zero unsafe completion, and no increase over one
  unnecessary clarification. Existing-file mutation without current evidence,
  no-effect progress, or same-check/same-epoch execution also disqualifies the
  candidate. Only a 0/3 objective result authorizes up to two retained general
  controller revisions; a nonzero result that fails a safety, mechanism, or
  clarification gate is falsified rather than selected.
- Paired confirmation: expand frozen control and candidate to three
  same-setting stability repeats per task. Require a positive paired objective
  delta, at least one task resolved in at least two of three candidate repeats,
  and no regression in completion-gate or refusal behavior. Do not treat
  deterministic repeats as independent population samples.
- Held tasks: compare on H02/H03/H05/H06/H07 without changing task prompts.
  Promotion requires a positive aggregate paired objective-completion delta,
  at least one evidence-only objective-and-contract pass, no loss of a
  control-passing contract, no increase in unnecessary clarifications, and
  zero unsafe completions or mechanism violations. Improvement therefore
  cannot be confined to the three tasks that selected the mechanism.
- Mechanism: existing-file mutations without a current observation, no-effect
  mutations counted as progress, and same-check/same-epoch executions must all
  be zero in the candidate traces.
- Reproducibility: corpus/model/protocol/context/temperature/server topology are
  fixed, with one slot and complete sampling/server provenance for confirmation;
  every trace verifies; all summaries are complete and infrastructure clean.

This establishes a realistic reliability baseline: objective success remains
primary, while the controller metrics explain whether a gain came from the
intended general mechanism.

## Follow-up wider-field gap audit — 2026-08-26

The two supplied analyses were treated as leads, not as executable
instructions. Their claims were cross-checked against the repository, the
retained traces, and primary sources. The follow-up found several gaps beyond
the original controller proposal:

- **Valid structure is not semantic success.** [The Constraint Tax](https://arxiv.org/abs/2605.26128)
  reports that hard schemas can raise validity while reducing answer or
  executable accuracy for small models, and [OrderBench](https://arxiv.org/abs/2607.18261)
  separately measures schema validity, exact semantics, constraint
  preservation, and unsafe acceptance. This supports Sprint 113's decision to
  score objective-and-contract completion independently of grammar validity and
  mechanism counters. A syntactically valid tool trace is not a passing task.
- **Combined constraints can suppress action selection.** The open-weight
  [tool-suppression study](https://arxiv.org/abs/2606.25605) reports that tool
  calls and structured output can each work independently yet interfere when
  deployed together. Ferric therefore must retain tool-attempt and termination
  metrics alongside output validity. A future two-pass or “constrain late” arm
  would change the inference protocol and needs its own intent and frozen
  comparison; it cannot be introduced as an unmeasured Sprint 113 fallback.
- **Schemas are a second instruction surface.** The
  [schema-description study](https://arxiv.org/abs/2608.08254) finds
  model-dependent prompt/schema weighting and large accuracy losses when their
  instructions conflict. Ferric's versioned evidence guidance and constrained
  action grammar need a single-source genealogy check so prompt text and schema
  descriptions cannot drift silently. That check remains a follow-up gap; the
  evaluated candidate is not changed after its revision budget.
- **Control structures compose rather than forming one universal agent loop.**
  The source-level [coding-agent scaffold taxonomy](https://arxiv.org/abs/2604.03515)
  finds that most surveyed agents combine multiple loop primitives. This
  reinforces keeping Evidence, planning, retry, and recovery as explicit,
  attributable mechanisms. It does not justify labeling Evidence execution as
  `evidence_planner` when no planner protocol exists.
- **One runtime should own causal projection and recovery.** Apache
  [Maka](https://github.com/apache/maka) independently uses an append-only
  record for model messages, tool calls/results, permission decisions, and
  termination, while its runtime owns control flow, projection, context, and
  recovery. This comparison supports Ferric's central `TraceStructure` and
  loop chokepoint, and exposes a remaining observability gap: prompt/schema
  genealogy should become durable trace provenance rather than an inferred
  property of a binary hash.
- **“Syntax check” must not mean “run an interpreter in the workspace.”**
  Python documents that `python -c` prepends the current directory to
  [`sys.path`](https://docs.python.org/3/using/cmdline.html#cmdoption-c), while
  the `site` module can import
  [`sitecustomize`](https://docs.python.org/3/library/site.html#sitecustomize).
  The [RustPython parser](https://rustpython.github.io/website/rustpython_parser/)
  provides an in-process lexical/parser boundary. This led directly to
  T-11309: candidate Python bytes are parsed, not executed, and the warning-only
  legacy behavior no longer depends on `PATH`.

Repository and runtime inspection added these non-literature gaps:

1. timed-out or controller-stopped traces needed to remain scoreable instead
   of disappearing from a screen;
2. feature-gated API, MCP, and ICM surfaces needed positive policy-propagation
   tests, not only the default CLI path;
3. the installed Sprint Loop now requires a tracked Book-v2 authority, so the
   legacy root ledgers had to be migrated without creating split-brain state;
4. Animus Launch still generated the legacy ledger shape and required
   deterministic Book-v2 scaffolds, hostile-input escaping, race-safe exclusive
   creation, exact tracked-file inventory, Cargo-safe names, and profile-specific
   ignores;
5. `run_check` remains available only when an operator supplies
   `--checks-file`; ordinary queries and newly launched projects have no
   verification command by default. Silently deriving arbitrary package
   scripts would violate Ferric's explicit execution-authorization boundary,
   so verification-by-default needs a new design—most plausibly reviewed,
   scaffolded named checks rather than implicit shell execution;
6. the controller has no requirement ledger linking task obligations to
   observations, mutations, and checks. The analyses' proposed
   `unaddressed → claimed → evidenced` state remains plausible but untested;
7. ordinary product traces have structural validation but no manifest or hash
   chain. Sprint 113's manually archived SHA-256 values protect this experiment,
   not every user trace;
8. syntax admission is deliberately bounded to Python today. Rust and
   JavaScript candidates receive no equivalent in-process pre-publication parse,
   and any extension must avoid turning validation back into implicit workspace
   execution; and
9. per-turn diffs, a session index, `--resume-last`, controlled trace forks,
   compaction latency telemetry, and explicit disposition of the unused
   planner/subagent policy fields remain operator-truthfulness and auditability
   work rather than delivered features.

These findings broadened the implementation and verification surface without
changing the pinned model, task prompts, selection gates, or two-revision cap.
Items 5–9 require new intent and evaluation authority after Sprint 113; they
are not reasons to reopen its falsified candidate or rejected planner arm.
