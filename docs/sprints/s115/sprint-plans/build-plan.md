Finalized - DO NOT EDIT

# Sprint 115 Build Plan

## Intents

- [INT-0007 — Hardware-calibrated autonomous development](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
  — state: active; acceptance criteria covered: AC-2, AC-3, AC-4, and AC-6.
- [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md)
  — state: planned; no acceptance criterion is claimed complete. T-11414 is
  enabling evidence toward AC-2 and AC-6, not satisfaction of the absent
  high-level run/resume workflow.

## Scope Boundaries

- The acquired Qwen3.8-27B UD-Q4_K_M artifact, frozen MH-RS01 seed, prompt,
  checks, grader, and allowed candidate paths do not change.
- No Codex edit may enter the candidate after the first Ferric invocation.
- `--trace-dir` is query-only. Other trace producers and the shared trace-sink
  helper retain their current behavior.
- Omitting `--trace-dir` retains `<workspace>/.ferric/trace`.
- Generated resume commands target PowerShell on Windows and POSIX `sh` on
  Unix. `cmd.exe` syntax is not promised. The documented shell must round-trip
  exact argv, not merely look quoted.
- This sprint does not claim INT-0008 AC-2 or AC-8. The full cross-platform
  `explain/run/status/resume/evidence/cleanup` workflow remains follow-up.
- Model execution begins only after the changed release binary, frozen
  harness, sandbox, and managed runtime are freshly qualified.

## Schema Tree

- Safely unblock the frozen app trial
  - Query trace boundary
    - T-11414: external trace root, explicit resume, and exact-argv hint
  - Fresh qualification
    - T-11501: product source and release binary
    - T-11502: lossless stale-state quarantine, frozen harness, and sandbox
    - T-11503: post-reboot resource and managed-runtime attestation
  - Causal application trial
    - T-11410: one-turn segment plus linked continuation, no Codex repair
  - Durable adjudication
    - T-11412: manifest, trace/effect audit, exact cleanup, and verdict

## Execution Sequence

### T-11414: Add a safe query-only external trace root and truthful resume surface

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/query.rs`,
  `crates/ferric-cli/tests/cli.rs`, `docs/basics-query.md`,
  `docs/commands.md`, `docs/configuration.md`
- **Depends on:** (none)
- **Acceptance criterion:** enabling prerequisite for INT-0008 AC-2 and AC-6;
  both high-level workflow criteria remain open.
- **Success criterion (EARS):**
  - **E14-A — WHEN** a fresh or ordinary resumed query omits `--trace-dir`,
    **THEN** Ferric **SHALL** retain the existing
    `<workspace>/.ferric/trace` allocation and compatibility behavior.
  - **E14-B — WHEN** a query supplies a valid external trace directory
    disjoint from its canonical workspace, **THEN** Ferric **SHALL** allocate
    the trace under that directory without creating workspace `.ferric`
    state.
  - **E14-C — WHEN** the requested root is equal to, above, or below the
    workspace; resolves through an existing file, symlink, or Windows reparse
    point; or aliases a Windows overlap by lexical or case semantics, **THEN**
    Ferric **SHALL** reject it before creating any directory, trace, or
    mock/model artifact.
  - **E14-D — WHEN** a valid external directory has been created, **THEN**
    Ferric **SHALL** repeat type, canonical equality, ancestor, descendant,
    symlink, and Windows-reparse checks before allocating the JSONL trace.
  - **E14-E — WHEN** an incomplete source trace was written outside the
    default workspace trace root, **THEN** resume without an explicitly
    repeated `--trace-dir` **SHALL** fail before mutation and resume with the
    same valid external root and workspace **SHALL** create a linked external
    continuation.
  - **E14-F — WHEN** Ferric prints a continuation or clarification instruction
    for a non-default workspace or external trace root, **THEN** it **SHALL**
    emit PowerShell syntax on Windows or POSIX-`sh` syntax on Unix that
    round-trips the exact `query`, `--resume`, `--workspace`, and `--trace-dir`
    argv for spaces, quotes, dollar signs, backticks, and separators.
- **Notes:** Resolve an absent tail from its deepest existing canonical
  ancestor. A test-only injection seam between directory creation and final
  validation must be unavailable in release builds and must prove every
  repeated E14-D predicate. Standard path APIs retain a documented concurrent
  local path-swap limitation; the path is operator-authored, never model input.

### T-11501: Qualify product source and the backend-enabled release binary

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md),
  [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** Rust source/tests/docs changed by T-11414,
  `docs/sprints/s115/control-artifacts/release/`, `target/release/ferric.exe`
- **Depends on:** T-11414
- **Acceptance criterion:** INT-0007 AC-2; enabling evidence toward INT-0008
  AC-6, which remains open until a high-level workflow drives the controls.
- **Success criterion (EARS):**
  - **E15-A — WHEN** T-11414 implementation is complete, **THEN** the release
    qualification **SHALL** record passing format, clippy, targeted and full
    tests plus the backend-enabled binary's source commit, SHA-256, version,
    and query-help surface before inference.
  - **E15-B — WHEN** the qualified binary is probed with default fresh/resume
    and external fresh/resume mock runs, **THEN** its observed paths, links,
    diagnostics, and help **SHALL** match T-11414's locked clauses without a
    workspace trace leak.

### T-11502: Quarantine stale state losslessly and re-prove the frozen harness and sandbox

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** `docs/sprints/s115/control-artifacts/harness/`,
  `target/s115-preserved-preflight/`, and these exact generated roots when
  present: `target/s114-experiment/app-harness`,
  `target/s114-experiment/self-test-workspaces`,
  `target/s114-experiment/app-workspace`, and
  `target/s114-experiment/launcher-attestation-probe`
- **Depends on:** T-11501
- **Acceptance criterion:** INT-0007 AC-3 and AC-4.
- **Success criterion (EARS):**
  - **E16-A — WHEN** any named stale root exists, **THEN** preparation **SHALL**
    resolve and verify the exact source and quarantine targets, manifest every
    entry's relative path, type, size, and SHA-256 where regular, move the
    entire root beneath `target/s115-preserved-preflight/` without recursive
    deletion, and prove pre/post manifest equality before continuing.
  - **E16-B — WHEN** all four canonical run roots are absent, **THEN** the
    harness gate **SHALL** re-prove every frozen input hash, expected positive
    and negative self-test result, standalone-Git candidate rule, and
    WSL/Bubblewrap network-disabled execution path, then inventory and
    losslessly re-quarantine every canonical root recreated by self-tests and
    re-prove all four roots absent immediately before T-11503; any failure
    **SHALL** stop as infrastructure before model inference.
- **Notes:** The quarantine is retained evidence, not disposable staging, and
  is not removed during Sprint 115 teardown.

### T-11503: Qualify the exact post-reboot managed runtime and freeze its handoff

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** `docs/sprints/s115/control-artifacts/runtime/` and owned Ferric
  local/global server runfiles
- **Depends on:** T-11502
- **Acceptance criterion:** INT-0007 AC-2.
- **Success criterion (EARS):**
  - **E17-A — WHEN** the post-reboot trial is prepared, **THEN** preflight
    **SHALL** record UTC observation/boot time; total and available physical
    and committed memory; GPU name, driver, total/used/free VRAM, utilization,
    temperature, and power state; every process whose resolved image equals
    the qualified Ferric or pinned llama-server plus each runfile-owned PID and
    their command lines; TCP `127.0.0.1:8080` plus every listener owned by those
    PIDs; `<project-root>/.ferric/server.json` and the exact path returned by
    `global_runfile_path()` with contents when present; model path/size/SHA-256;
    engine path/version/SHA-256; and WSL distribution/version/state,
    Bubblewrap version, and no-network probe result, without stopping unrelated
    user applications.
  - **E17-B — WHEN** the exact Q4_K_M/32,768-context/24-GPU-layer coordinate is
    started through Ferric's managed runtime, **THEN** the attestor **SHALL**
    bind ownership, arguments, health, model identity, effective properties,
    grammar nonce smoke, and bounded throughput, or stop with a truthful typed
    classification before candidate inference.
  - **E17-C — WHEN** runtime qualification succeeds, **THEN** that owned server
    and its identities **SHALL** become immutable T-11410 inputs without an
    unauthorized coordinate fallback, second download, or unrecorded restart.
- **Notes:** Sprint 114's 3.2065 tokens/s is comparison-only.

### T-11410: Execute the frozen MH-RS01 application through a forced linked continuation

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** `target/s114-experiment/app-workspace`,
  `target/s114-experiment/app-traces`, and
  `docs/sprints/s115/control-artifacts/app-run/`
- **Depends on:** T-11503
- **Acceptance criterion:** INT-0007 AC-3 and AC-4.
- **Success criterion (EARS):**
  - **E10-A — WHEN** the sealed seed, qualified binary/runtime, external trace
    root, checks file, prompt, sampling, and one-turn budget are bound, **THEN**
    Ferric **SHALL** receive the exact frozen task and retain invocation, trace,
    and candidate provenance.
  - **E10-B — WHEN** the first segment ends incomplete at its one-turn budget,
    **THEN** the operator **SHALL** issue the exact linked continuation with
    explicit workspace and trace root and a 27-turn budget; any other stop
    reason **SHALL** be retained and classified rather than rewritten.
  - **E10-C — WHEN** the model mutates the candidate, **THEN** only Ferric's
    authorized tools **SHALL** change candidate bytes and every observed tree
    change **SHALL** reconcile with trace effects and the command journal.
  - **E10-D — WHEN** model-authored code or a model-visible check executes,
    **THEN** it **SHALL** run only through the frozen network-disabled sandbox;
    sandbox unavailability **SHALL** fail closed as infrastructure.
  - **E10-E — WHEN** the run reaches a terminal state, **THEN** the frozen
    grader **SHALL** publish all seven dimensions, final in-session check
    freshness, structural trace validity, and distinct application/model/
    harness/infrastructure outcomes without Codex candidate repair.
- **Notes:** After E10-A begins, Codex may inspect and copy evidence but may not
  write, format, reset, or repair the candidate.

### T-11412: Archive the experiment, verify exact teardown, and publish the capability verdict

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md),
  [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `docs/sprints/s115/control-artifacts/`,
  `docs/sprints/s115/sprint-tests/`, `docs/work/tasks.md`,
  `docs/work/completed-tasks.md`, both linked intent chapters,
  `docs/sprints/s115/sprint-meta.md`, and the exact disposable roots
  `target/s114-experiment/app-harness`,
  `target/s114-experiment/self-test-workspaces`,
  `target/s114-experiment/app-workspace`,
  `target/s114-experiment/app-traces`, and
  `target/s114-experiment/launcher-attestation-probe`
- **Depends on:** T-11410
- **Acceptance criterion:** INT-0007 AC-6; enabling closeout evidence toward
  INT-0008 AC-6, which remains open with the high-level workflow.
- **Success criterion (EARS):**
  - **E12-A — WHEN** application execution stops, **THEN** an independently
    verifiable manifest **SHALL** bind source, binary, model, engine, prompt,
    frozen inputs, invocations, traces, effects, journal, final workspace,
    grades, runtime evidence, and every failed or partial attempt.
  - **E12-B — WHEN** every retained trace and candidate snapshot is audited,
    **THEN** trace verification, manifest hashes, allowed-path policy, and
    effect/tree reconciliation **SHALL** either pass or name every mismatch.
  - **E12-C — WHEN** evidence is archived and the owned server is no longer
    required, **THEN** teardown **SHALL** verify the five exact disposable
    roots above are absent and leave no owned process, listener, or local/
    global runfile while preserving `models/`, committed sprint evidence, and
    `target/s115-preserved-preflight/`.
  - **E12-D — WHEN** Sprint 115 closes, **THEN** the Book **SHALL** distinguish
    evaluated capability from application success, update INT-0007 only when
    its evidence criteria are met, keep INT-0008 non-realized, and add a named
    ordered backlog task for the full compact workflow.
- **Notes:** T-11412 remains the Book's archive/verdict task, not the larger
  workflow feature.
