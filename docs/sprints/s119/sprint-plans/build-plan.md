Finalized - DO NOT EDIT

# Sprint 119 Build Plan

## Intents

- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — active;
  affected criterion: AC-6 source-driven bounded execution and truthful cleanup;
  enabling coverage only for AC-9, not full workflow acceptance.

## Schema Tree

- Review and refactor source-owned subprocess execution
  - T-11901: shared bounded process scope
  - T-11902: consolidate test adapters and exact cleanup regressions
  - T-11903: Cargo-only Linux CI and truthful sprint closeout

## Execution Sequence

### T-11901: Consolidate benchmark and test process ownership

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-process/`, workspace and consumer Cargo manifests,
  lockfile, `crates/ferric-bench/src/{process,runner,verify,autonomy,lib}.rs`.
- **Depends on:** none.
- **Acceptance criterion:** AC-6.
- **Success criterion (EARS):**
  - **E01:** **WHEN** a controlled child succeeds, times out, leaves descendants
    in its owned scope, or its Rust owner unwinds, **THEN** the shared boundary
    **SHALL** prove scope cleanup within a separate five-second cleanup bound
    before successful return; it shall not equate leader exit with scope exit.
  - **E02:** **WHEN** native spawn fails after creating a Windows child,
    **THEN** the boundary **SHALL** retain ownership through termination/reaping
    and return failure without leaving the child suspended or runnable.
  - **E03:** **WHEN** output exceeds the selected head/tail capture limits or
    descendant writers remain open, **THEN** collection **SHALL** stay bounded
    in memory and time and preserve exit versus timeout classification.
- **Notes:** Reuse guarded Windows Job assignment and query active accounting;
  remove command-group. Unix scope means the controlled process group, not a
  security sandbox against deliberate group escape. Linux adopted reaping must
  be scoped, never `waitpid(-1)`. File-backed capture bounds retained memory,
  not generated disk bytes; disk quotas/hostile output containment are deferred.
  Shared crate must not silently install production-wide parent watchers or
  subreapers without an explicit documented API/consumer opt-in.

### T-11902: Make source-driven test cleanup one coherent contract

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** CLI test containment modules, source lifecycle fixture,
  `server_process.rs`, `server_registration.rs`, test helpers in `server.rs`,
  `tests/{cli,bench_mock,server_lifecycle_fixture}.rs`, and carried-over
  `main.rs`, `query.rs`, `cron.rs` edits.
- **Depends on:** T-11901 for shared API; independent fixture review can precede integration.
- **Acceptance criterion:** AC-6; model-free lifecycle enabling evidence for AC-9.
- **Success criterion (EARS):**
  - **E04:** **WHEN** CLI/benchmark/lifecycle source tests run, **THEN** they
    **SHALL** preserve existing assertions and use bounded owned child cleanup;
    batch pipe collection shall not wait indefinitely on inherited writers.
  - **E05:** **WHEN** an exact upstream owner exits in a controlled source
    regression, **THEN** its owned helpers **SHALL** exit and be proved reaped
    before success, while the positive managed-server lifecycle still permits
    its intentional launcher-to-server lifetime handoff.
  - **E06:** **WHEN** exact-parent observation or shutdown registration races
    normal cleanup, **THEN** it **SHALL** retain the native descriptor through
    the watch lifetime, reject invalid poll events, serialize registry removal
    with signalling, and refuse new ownership after shutdown begins.
- **Notes:** Do not pair an immediate SIGKILL on a watcher owner with a promise
  that the same watcher will later clean nested groups. Controlled Linux
  fixtures must retain a source supervisor/reaper or namespace outer boundary.
  General arbitrary group escape/owner-SIGKILL containment remains a documented
  backlog item, not an accepted property of POSIX groups. No manual process
  cleanup may rehabilitate a failed test. Remove unsupported pidfd folklore.

### T-11903: Make verification source-driven and close the actual sprint

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `.github/workflows/ci.yml`, source verification scripts/tests if
  needed, `AGENTS.md`, process-contract docs, Book sprint/work/intent evidence.
- **Depends on:** T-11901, T-11902.
- **Acceptance criterion:** AC-6.
- **Success criterion (EARS):**
  - **E07:** **WHEN** the Linux lifecycle CI gate runs, **THEN** it **SHALL**
    invoke `cargo test` within the existing isolated non-root PID/network
    boundary, retain reaping/teardown and exit-status propagation, and never
    extract or directly execute a compiled target artifact.
  - **E08:** **WHEN** Sprint 119 is offered for merge, **THEN** its evidence
    **SHALL** map every locked clause to actual results, record the independent
    Test critique plus extra post-Loop adversarial phase audit, and offer one
    confirmed `dev` to `main` PR containing only this sprint's commits.
- **Notes:** Sprint 118 is already merged. Its addendum records carryover,
  never retroactive green evidence. Owner alone merges. Preserve the unrelated
  Sprint 114 evidence edit byte-for-byte; any gate exception is explicit.
