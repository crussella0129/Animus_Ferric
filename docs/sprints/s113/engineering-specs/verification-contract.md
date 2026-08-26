# Sprint 113 Approved Verification Contract — Causal Harness Evaluation

Approved by the user on 2026-08-02. This is a repository-native,
tool-independent verification contract, not an IDE artifact or executable
workflow. A dated addendum may clarify a discovered test seam, but may not
weaken a gate after results are observed.

## 1. Model-free unit evidence

- Navigation: literal zero results, normalized roots/paths, full and paginated
  reads, CRLF/trailing-newline/empty files, accurate ranges, stable full SHA,
  and honest truncation/completeness.
- Mutation preparation: create/modify/delete/no-effect classification, stale
  preimages, external appearance, same-turn evidence denial, partial effects,
  and supported syntax transition matrix.
- Controller: evidence coverage, one-epoch-per-real-effect, failed-check
  fingerprints, monotonic attempts, unchanged-check refusal, repair barriers,
  and conservative resume invalidation.
- Result math: two-arm coordinates, completeness, deterministic AB/BA order,
  infrastructure-unpaired exclusion, and per-policy aggregates.

## 2. Registry and loop integration

- Prove the registry chokepoint produces typed observations/effects rather than
  deriving policy from tool names or prose.
- Exercise blind/stale mutation rejection across write, edit, multi-edit,
  patch, and structural tools; unknown model mutation fails closed.
- Exercise the complete repair path: read → mutate → failed check → later-turn
  inspection → material repair → passing check → completion.
- Assert repeated same-name/same-epoch checks never spawn a second process.
- Assert controller rejection precedes human/sink approval and admitted calls
  prompt each required approver once.

## 3. Trace, replay, and recovery

- Positive and malformed cases cover policy compatibility, controller base,
  call-ID/tool/check matching, outcome agreement, evidence causality, epoch
  integrity, repair causality, checkpoint parity, and packet truth.
- Verify pause, resume, resume-of-resume, clarification answer, generic goal
  amendment, compaction, pre-dispatch crash, ambiguous dispatched/no-result
  crash, and errored calls with measured partial effects.
- Run the same central `TraceStructure` decisions through `ferric trace verify`
  and prove valid and invalid verification is workspace side-effect-free.

## 4. CLI and compatibility

- Test fresh policy selection, legacy default, resume inheritance, mismatch
  refusal, and propagation across query/API/MCP/chat/ICM.
- Check evidence prompt guidance is general/versioned and absent from legacy.
- Verify old trace/runfile/result fixtures deserialize with safe defaults and
  new event rendering does not expose hidden bytes.
- Confirm control and candidate child argv are identical except executable and
  the candidate-only policy flag.
- Confirm retained trace paths cannot collide across arms/segments/trials.

## 5. Rust quality gates

Run affected packages after each build unit, followed by:

```text
cargo fmt --check
cargo test -p ferric-core
cargo test -p ferric-trace
cargo test -p ferric-tools
cargo test -p ferric-loop
cargo test -p ferric-bench
cargo test -p ferric-cli --features backend-openai
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo clippy -p ferric-cli --features backend-openai --all-targets -- -D warnings
```

The pushed head must also receive the repository's independent CI matrix. Mock
or offline smoke tests may find regressions but are not model evidence.

## 6. Real-model development screen

Use only the exact immutable control inputs:

- GGUF: `qwen2.5-coder-7b-instruct-q4_k_m.gguf`
- SHA-256: `509287F78CB4D4CF6B3843734733B914B2C158E43E22A7F4BF5E963800894D3C`
- context 8192, constrained grammar protocol, temperature 0
- recovery tasks H01, H04, H08
- frozen control executable from commit `cabe2368`

Launch through `ferric server up`. Independently verify PID command line,
listener owner, `/health`, `/v1/models`, engine/model hashes, runfile metadata,
and one-slot configuration before queries.

Run evidence once per task. Advancement requires at least 1/3 objective
completions, complete infrastructure-clean rows, valid retained traces, zero
admitted blind/stale mutations, zero admitted no-effect mutations, and zero
second check-process executions at an unchanged epoch. If attempt one remains
0/3, allow at most two retained trace-justified revisions to general controller
behavior. Do not inspect held-task traces while revising.

## 7. Frozen paired confirmation

Freeze and hash the candidate. Run legacy and evidence on H01/H04/H08 for three
same-setting stability repeats, adjacent and counterbalanced AB/BA, each in a
fresh workspace: 18 total rows. Temperature zero is greedy, so repeats measure
reproducibility rather than independent Bernoulli trials.

Promotion requires a positive paired objective-completion delta and evidence
completion of at least one task in at least 2/3 repeats, with no contract,
clarification, safety, mechanism, infrastructure, trace, or lifecycle
regression. Dirty/unpaired infrastructure rows are excluded, never scored as a
model loss.

## 8. Untouched held tasks

After candidate freeze, run H02, H03, H05, H06, and H07 recovery once per arm
on fresh workspaces. Report all ten rows and pair outcomes. This tests transfer
beyond the three development trajectories but does not justify a broad
population-accuracy claim.

## 9. Teardown and evidence retention

Before shutdown, revalidate the registered PID, executable, model command line,
listener, health, and model endpoint. Use `ferric server down`, then prove the
PID/listener/matching server process/runfiles are absent and status is down.

Retain commands, timestamps, hashes, row JSON, summaries, trace digests,
side-effect-free verifier results, and lifecycle evidence in the ignored Sprint
113 test report. Durable tracked records must use template-safe identifiers.
