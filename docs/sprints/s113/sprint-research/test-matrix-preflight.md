# Sprint 113 Test Matrix Preflight

This is a pre-approval coverage map, not the approved verification contract. The
approved verification contract carries these gates and adds exact implemented symbols
without weakening them.

## Evidence hierarchy

1. Pure/unit tests prove formatting, hashing, classification, candidate
   construction, and controller decisions without a model.
2. Registry and loop integration tests prove the events and workspace effects
   produced through the real dispatch chokepoint.
3. Malformed-trace/replay tests prove durable state cannot claim evidence the
   model never received.
4. CLI/mock tests prove flags, resume inheritance, child argv, scheduling,
   provenance, and result aggregation across process boundaries.
5. Real-server Qwen runs prove whether the complete harness changes objective
   completion on development and untouched held tasks.

Aggregate objective success cannot substitute for a failed mechanism or safety
gate.

## Tool and controller requirements

| Requirement | Authoritative test evidence |
| --- | --- |
| Explicit literal zero-result navigation | `find_files`/`search_files` tests assert nonempty output includes normalized root, literal query/pattern, zero count, and no glob interpretation |
| Stable file identity | identical bytes through full and paginated `read_file` calls yield the same full SHA-256 and normalized path |
| Accurate ranges | empty file, trailing newline, CRLF, one-line, partial, out-of-range, start-only, end-only, and invalid range cases assert total/returned ranges |
| Truncation honesty | a registry-truncated read is marked incomplete and establishes no coverage even though the trace retains full output |
| Partial-read accumulation | non-truncated ranges merge only under the same full digest; drift resets coverage |
| Same-batch information boundary | native multi-call read→edit is blocked; the equivalent next-turn edit is admitted |
| Blind existing-file mutation | write/edit/multi-edit/patch against an unobserved existing target are blocked and byte-identical |
| New-file creation | an actually absent target may be created; external appearance between prepare/commit blocks overwrite |
| Current-hash revalidation | external content change after observation but before mutation produces stale-evidence block and no write |
| No-effect rejection | identity replacement, identical write, net-zero multi-edit, net-zero patch, existing directory creation, and equivalent structural operation produce no write/effect/epoch |
| Syntax admission | valid→invalid and absent→invalid Python block atomically; invalid→invalid changed and invalid→valid are admitted; unsupported/missing validator is explicitly recorded |
| Structural mutation safety | copy/move/delete destination/source requirements and real path deltas are asserted; recursive unmodeled operations fail closed |
| Opaque-tool safety | a custom write/execute tool with no control metadata cannot bypass evidence mode |
| Human approval order | controller-rejected action never invokes accept-edits or sink approvers; admitted mutation invokes each required approval exactly once |
| Actual-effect epoch | unchanged/error-without-effect does not advance; one call changing multiple paths advances once; an error with measured partial effect advances once |
| Failed-check identity | normalized CRLF/workspace-path diagnostics produce a stable lowercase 64-hex digest and monotonic per-name attempt |
| No unchanged recheck | same named check/same epoch is blocked before process spawn; a real mutation permits one new execution |
| Repair inspection | failed check→immediate mutation blocks; later-turn reread→materially changed mutation→check pass succeeds |
| Authored evidence | successful mutation supplies exact postimage evidence for a later turn, but not a co-proposed same-turn call and not after a failed check |

Primary test locations:

- `crates/ferric-tools/tests/builtin_file_tools.rs`
- registry unit tests in `crates/ferric-tools/src/registry.rs`
- syntax unit tests in `builtin/check_syntax.rs`
- new pure controller tests in `crates/ferric-loop/src/controller.rs`
- `crates/ferric-loop/tests/accept_edits.rs`
- `crates/ferric-loop/tests/verification_gate_tests.rs`
- new `crates/ferric-loop/tests/evidence_controller_tests.rs`

## Trace, replay, and recovery requirements

Each malformed sequence must fail through shared `TraceStructure`, and the CLI
`trace verify` test must confirm it reaches that shared decision without
touching the workspace.

| Required invariant | Positive and negative cases |
| --- | --- |
| Policy compatibility | literal old policy defaults to legacy; fresh evidence trace records evidence; explicit resume policy switch rejects |
| Controller base | fresh evidence session requires one initial controller checkpoint; legacy rejects controller-only events; duplicates/missing base reject |
| Call evidence ordering | observation/block/check/effect must match active call ID/name and occur before its result; out-of-turn/after-result/mismatched cases reject |
| Outcome agreement | observation requires success; block requires error/no effect; pass requires success; fail requires error; partial effect may coexist with error |
| Evidence causality | effect before observation, stale hash, partial coverage, same-turn observation, or mismatched before digest rejects |
| Epoch integrity | real effect advances exactly once; skipped/duplicate/future epochs reject; multi-path effect still advances once |
| Check uniqueness | duplicate actual `(name, epoch)` execution rejects; a recorded unchanged-check block is valid |
| Repair causality | mutation after failure without a later-turn qualifying observation rejects |
| Checkpoint parity | controller checkpoint policy/epoch/checks/ledger exactly match projected state; unsupported version rejects |
| Resume pairing | every evidence recovery checkpoint has its controller checkpoint in the canonical order; pause requires both |
| Recovery packet truth | packet matches pause/check/changed-path/reread facts and its literal message is replayed byte-for-byte |
| Clarification ordering | explicit clarification answer anchors first and omits generic `needs_input` pause prose; generic amendment retains recovery packet |
| Resume-of-resume | repair state and policy persist, file evidence is stale, and controller/core anchors remain paired |
| Compaction independence | contradictory or missing model summary cannot remove or alter controller facts |
| Crash prefixes | pre-dispatch retry remains safe; dispatched/no-result mutation remains ambiguous; no invented observation/effect appears |

Primary test locations:

- `crates/ferric-trace/src/lib.rs`
- `crates/ferric-loop/src/trace_structure.rs`
- `crates/ferric-loop/src/replay.rs`
- `crates/ferric-loop/tests/recovery_protocol_tests.rs`
- `resume_tests.rs`, `clarification_tests.rs`, `compaction_tests.rs`
- `crates/ferric-cli/src/trace_verify.rs`
- `crates/ferric-cli/tests/cli.rs`

## CLI and product-surface requirements

| Requirement | Evidence |
| --- | --- |
| Fresh default/override | CLI parser and `build_run_config` select the requested policy; fresh default remains deliberate and tested |
| Resume inheritance | omitted policy inherits trace; explicit mismatch returns nonzero before opening a new trace or touching workspace |
| Shared surfaces | query, API, MCP, chat escalation, and ICM pass the same policy into `RunArgs`; direct human `chat !run` remains outside model control |
| Prompt guidance | fresh evidence `SessionPrompt.system` contains versioned general guidance; legacy prompt does not; no task text appears |
| Trace rendering | `trace cat` renders every new event and fingerprints without dumping hidden candidate bytes |
| Side-effect-free verify | file tree hash before/after `trace verify` is identical for valid and invalid traces |
| Server argv | llama command includes requested seed/parallel; Ollama rejects or explicitly ignores without claiming provenance |
| Runfile compatibility | old runfile without new fields deserializes; new runfile preserves engine build/config metadata |
| Server lifecycle | managed up→health→models→query→status→down proves exact PID/model/listener and leaves no matching process/listener/runfile |

## Autonomy runner and result-schema requirements

| Requirement | Evidence |
| --- | --- |
| Orthogonal coordinate | harness policy is separate from autonomy variant in child argv, trace, row, provenance, and summary |
| Frozen control compatibility | control child receives no unknown policy flag; candidate child receives explicit evidence flag; all other shared argv are byte-identical |
| Binary attribution | binaries are canonicalized, copied, sized, hashed, version-probed, and distinct before episodes |
| Pair schedule | per coordinate arms are adjacent and deterministic AB/BA; fresh workspaces/profile dirs are distinct |
| Trace retention | filenames include arm/task/variant/trial/segment and cannot overwrite; every digest rehashes correctly |
| Completeness | missing/duplicate `(policy, task, variant, trial)` coordinates make summary incomplete |
| Policy provenance | row policy agrees with every segment's `PolicySelected`; mixed per-policy binaries do not masquerade as one run-global binary |
| Pair scoring | legacy-only/evidence-only/both/neither use complete infrastructure-clean partners only; dirty/unpaired rows are separate |
| Existing aggregates | pass-power and repository-brief comparisons remain partitioned by harness policy |
| Mechanism metrics | blocks, observations, check executions/failures, effects, repair attempts, and packets aggregate exactly from traces |
| Planner fail-closed | evidence-planner is rejected until its declared trace-role orchestration exists; no evidence-only fallback is labeled planner |

Primary test locations:

- `crates/ferric-bench/src/runner.rs`
- `crates/ferric-bench/src/autonomy_results.rs`
- `crates/ferric-bench/src/summary.rs`
- `crates/ferric-cli/src/autonomy_cmd.rs`

## Rust gates

Run affected packages after each coherent build unit, then the repository gates:

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

CI must independently confirm the Windows/Linux fmt+clippy+workspace matrix,
backend-openai clippy job, and aarch64 type check at the pushed head SHA.

The offline `tools/demo-smoke.ps1` is not evidence for this sprint's model
claim. It may catch unrelated product regressions, but it cannot replace the
real Ferric-managed server runs below.

## Real-model development screen

Exact immutable inputs:

- model:
  `C:\Users\<you>\Animus_Ferric\models\qwen2.5-coder-7b-instruct-q4_k_m.gguf`
- model SHA-256:
  `509287F78CB4D4CF6B3843734733B914B2C158E43E22A7F4BF5E963800894D3C`
- parameters: `7.615616512B`
- context: `8192`
- protocol: grammar/constrained JSON
- temperature: `0`
- tasks/variant: H01, H04, H08 / recovery
- frozen control executable:
  `sprints/s113/control-artifacts/ferric-control-cabe236.exe`

Launch the exact model through the candidate `ferric server up`, with CPU
offload configuration matching the control and one recorded slot. Independently
verify PID command line, health, `/v1/models`, model path/hash, listener owner,
and server build before querying.

Screen one evidence row per task. Promotion to paired confirmation requires:

- at least 1/3 objective completions
- complete and infrastructure-clean rows
- all retained traces structurally verify
- zero admitted blind/stale existing-content mutations
- zero admitted no-effect mutations
- zero repeated named-check process executions at the same epoch
- no task-specific prompt or widened arbitrary execution

If screen attempt one remains 0/3, retain it and permit no more than two
trace-justified revisions to general controller/interface behavior. Never tune
against held tasks. Freeze and hash the candidate before confirmation.

## Paired confirmation

Run the frozen legacy binary and frozen candidate on fresh workspaces for each
H01/H04/H08 recovery coordinate, counterbalancing arm order across three
same-setting stability repeats (18 rows). Both connect to the same verified,
managed real server configuration.

Required outcome gate:

- positive paired objective-completion delta
- evidence completes at least one task in at least 2/3 stability repeats
- no contract, clarification, safety, mechanism, infrastructure, trace, or
  lifecycle regression

Do not report Wilson intervals over the three deterministic repeats as if they
were independent samples.

## Untouched held-task comparison

After candidate freeze, run H02/H03/H05/H06/H07 recovery once per arm on fresh
workspaces. Report every row and paired outcome. These five tasks test whether
the intervention generalizes beyond the three development trajectories; they
still do not justify a broad population-accuracy claim.

## Teardown evidence

Before shutdown, validate the runfile PID, executable identity, command-line
model, listener ownership, health, and model endpoint. Use `ferric server down`,
then independently prove:

- PID absent
- listener absent
- no matching model server process remains
- runfile absent
- subsequent `server status` reports no server

Retain commands, timestamps, hashes, result rows, summaries, and trace verifier
outcomes in the ignored Sprint 113 test report, then record durable conclusions
without machine identity in ADR/task history.
