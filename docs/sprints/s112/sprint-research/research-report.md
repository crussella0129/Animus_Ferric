# Sprint 112 Research Report

## Question

What prevents the current Ferric loop from being a reliable autonomous coding
agent on ambiguous, long-horizon repository work, and what baseline would let a
future sprint prove improvement rather than rely on a successful demo anecdote?

## Frozen pre-change state

- Git commit: `1b0c0dfef52400d2686b9e69c1e6e623da71bfda`
- `origin/main`, `origin/dev`, and local `dev` were identical and the worktree
  was clean when the sprint started.
- The retained live result is one Qwen2.5-Coder-7B six-turn task completed in
  about 58 seconds through a real `ferric server`. That proves the path works;
  one task does not estimate general accuracy.

## Runtime findings

1. **Recovery is currently one-shot.** Every stop writes `SessionEnd`, replay
   rejects any trace with `SessionEnd`, and a resumed run does not emit a new
   `SessionPrompt`. A second interruption therefore cannot be resumed.
2. **Replay does not bind a trace to its repository.** `SessionStart` records a
   workspace, but `ReplayedState` discards it and CLI/MCP resume validate only
   the action protocol. A trace from repository A can be resumed in repository
   B.
3. **Tool side effects and replay history are not crash-consistent.** `TurnEnd`
   is durable before dispatch, while tool results and mutations occur later.
   Replay infers commitment from the following `TurnStart` and drops an open
   tail. A crash after a write can leave disk ahead of the reconstructed model
   history.
4. **Recovery forgets control state.** Guard windows, nudge state, compaction
   boundaries, truncation state, and token accounting are reconstructed only
   partially or reset.
5. **Ambiguity has no non-success continuation.** The structured protocol has
   only `task_complete` and `submit_plan`; a native-text question can be
   reported as successful final text, while constrained models must guess or
   fail parsing.
6. **Completion is assertion-based.** The model can call `task_complete`
   without current executable evidence that the repository still passes an
   operator-approved check after its latest mutation.
7. **Progress guards judge call shape before results.** Legitimate distinct
   reads or changing status polls can resemble repetition, while repeated calls
   with no state change can look productive until too late.

## Baseline findings

1. L0-L6 currently records one observation per level and reduces calibration to
   the highest passing level, flattening intermittent and non-monotonic failure.
2. A spec's `max_turns` is not enforced by the spawned query.
3. The runner inherits user/model-profile state, does not pin deterministic
   sampling, and can measure a profile produced by a prior calibration.
4. Child stdout/stderr are piped but drained after exit, which can deadlock a
   verbose child; streaming is not disabled.
5. Higher-level graders inspect shallow files/regexes instead of executing the
   generated artifact. Completion can pass without an exact successful terminal
   event.
6. Results omit trial identity, commit/binary/model hashes, server settings,
   cold/warm state, trace path, and several guard/tool diagnostics. Malformed
   rows are silently discarded.
7. Toolbench promotes on a pooled point estimate. A weak individual tool can be
   hidden by strong peers, and ten trials cannot establish a 90% reliability
   lower bound.

## Baseline design

The internal baseline is a versioned task matrix, not a claim of external
general-agent performance:

| Block | Initial size | What it estimates |
|---|---:|---|
| Repository ambiguity | 8 tasks | clarification precision/recall and safe pause |
| Recovery | 8 tasks | resume, resume-of-resume, reconciliation, workspace refusal |
| Long horizon | 8 tasks | verified completion and failure distribution |
| Policy variants | 3/task | current loop, recovery-enabled, optional repository brief |

This produces 72 deterministic nightly episodes once the corpus exists. The
first operational baseline also retains three cold server lifecycles, ten demo
repeats, ten complete L0-L6 blocks, and Ring-0 tool trials. Report per-task and
per-tool rates with Wilson 95% intervals; do not promote from a pooled average.

Required metrics:

- resolved-at-1 and pass^3;
- recovery success, resume-of-resume success, and duplicate/collateral effects;
- clarification precision, recall, and unnecessary-question rate;
- executable check result after the latest mutation;
- turns, tokens, tool calls, wall time, and 80%-horizon position;
- guard, truncation, provider/tool error, pause, and terminal-reason
  distributions;
- run/trial ID, timestamp, commit and binary hash, model identity/hash, engine
  version/flags, sanitized host profile, and retained trace.

## Statistical interpretation

Ten successes are a useful repeatability signal, not proof that true reliability
is at least 90%. A 10/10 Wilson 95% interval is approximately 72.2%-100%; 40/40
is approximately 91.2%-100%. Temperature-zero repeats of one prompt estimate
operational repeatability, not task generalization.

## External context

- Anthropic recommends clean isolated environments, end-state grading, and
  pass^k rather than anecdotal traces:
  <https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents>
- OpenAI reported in July 2026 that roughly 30% of SWE-bench Pro tasks were
  broken and withdrew it as a reliable frontier measure, reinforcing the need
  to audit benchmark tasks before interpreting scores:
  <https://openai.com/index/separating-signal-from-noise-coding-evaluations/>
- SWE-bench-Live includes Rust and Windows tasks and is a better eventual
  external freshness check than treating the internal harness as a public
  benchmark: <https://swe-bench-live.github.io/>
- SWE-smith reports 15.2% SWE-bench Verified for an agent-specialized 7B model;
  that is evidence that small models are not automatically crushed, but it is
  not comparable to Ferric's single retained task:
  <https://arxiv.org/html/2504.21798v2>

## Decision

Sprint 112 will ship the minimum recovery spine and a trustworthy internal
measurement layer. It will call the result a **general-autonomy candidate**, not
a reliable general autonomous coding agent. External SWE-bench-Live work,
kernel sandboxing, unconstrained shell access, and broad multi-agent planning are
out of scope.

