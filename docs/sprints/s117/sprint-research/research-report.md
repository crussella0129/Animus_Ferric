# Sprint 117 Research Report

## Intents Reviewed

- [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md) — selected. T-11606 is the explicit failed-close gate before the remaining local-model work. Relevant acceptance criteria are AC-3 (explicit partial/stale state), AC-4 (truthful status), AC-6 (exact ownership), and AC-7 (bounded cleanup). Current state: active; the Sprint 116 implementation is useful but clause-level acceptance is not complete.

No new intent is required. Sprint 117 is remediation work already owned by INT-0008 and recorded as T-11606.

## 1. Sprint Goal

Close T-11606 by making the identity-safe server lifecycle both correct and
provable at the nineteen finalized E01-A through E05-D clauses from Sprint 116.
The sprint must retain the sound native Windows HANDLE and Linux pidfd design,
fix the contract divergences found below, route every lifecycle consumer
through a shared typed discovery result, add narrow deterministic seams around
process/listener/store side effects, and execute the feature-gated lifecycle
fixture on Windows and Linux CI.

This sprint does **not** restore Tailscale Serve mutation (T-11510), calibrate a
model, or build the compact front door. It proves the ownership boundary those
later tasks depend on. `--tailscale` remains an early, zero-side-effect refusal.

The Sprint 116 failure report also records a process checkpoint failure. That
historical report remains immutable. The owner subsequently approved the
correction, PR #103 merged, and Sprint 117 starts from merge head
`baebc04686e04df510b8127970d6c19eb30145e0`; this resolves only the checkpoint
history, not any of the technical acceptance gaps.

## 2. Existing Code Survey

| File | Relevance | Notes |
|---|---|---|
| [`docs/intents/INT-0008-unified-local-model-workflow.md`](../../../intents/INT-0008-unified-local-model-workflow.md) | Governing intent | T-11606 gates later model/runtime/front-door work; AC-3/4/6/7 remain the acceptance boundary. |
| [`docs/work/tasks.md`](../../../work/tasks.md) | Ordered work state | T-11606 explicitly requires tagged start tokens, finalized E01/E03 matrices, injected E02/E04/E05 seams, real concurrency, complete output assertions, hardened fixtures, two-OS CI, and immutable clause evidence. |
| [`docs/work/completed-tasks.md`](../../../work/completed-tasks.md) | Prior implementation record | T-11504 is correctly recorded as partial rather than accepted; remediation links to T-11606. |
| [`docs/sprints/s116/failure-report.md`](../../s116/failure-report.md) | Failed-close handoff | Establishes that implementation value did not substitute for clause-level evidence; one of nineteen finalized test names existed and the feature fixture was not in normal CI. |
| [`docs/sprints/s116/sprint-plans/build-plan.md`](../../s116/sprint-plans/build-plan.md) | Frozen contract | Owns the nineteen EARS clauses E01-A through E05-D. Sprint 117 must not silently weaken them to fit current behavior. |
| [`docs/sprints/s116/sprint-plans/test-plan.md`](../../s116/sprint-plans/test-plan.md) | Frozen evidence map | Supplies the nineteen executable acceptance names plus transition, native-smoke, CLI, and fixture expectations. |
| [`docs/sprints/s116/sprint-tests/critique.md`](../../s116/sprint-tests/critique.md) | Independent rejection | Correctly rejects aggregate test counts and narrative claims as substitutes for exact clause-to-command evidence. |
| [`crates/ferric-cli/src/server_registration.rs`](../../../../crates/ferric-cli/src/server_registration.rs) | E01/E04/E05 store | Lossless captures, no-clobber mirrored publication, and compare-and-remove are a strong base. Validation accepts any nonblank start token, absent slots are later dropped, publication fault injection is incomplete, and orchestration collapses cleanup outcomes to booleans/strings. The apparent `select_unique` typed selection is test-only and does not serve production consumers. |
| [`crates/ferric-cli/src/server_resolution.rs`](../../../../crates/ferric-cli/src/server_resolution.rs) | E03 resolver | Exact process and registration keys gate aliasing, but typed scope/path and conflict-versus-unverifiable distinctions collapse into labels and `Vec<String>`. |
| [`crates/ferric-cli/src/server_process.rs`](../../../../crates/ferric-cli/src/server_process.rs) | E02 native ownership | Windows duplicates/retains a process HANDLE; Linux uses pidfd for inspect/signal/wait. Producers already emit tagged tokens. Linux argv decoding is lossy, wildcard state is currently documented as teardown-authorizing, and unreadable shared-socket owners can be skipped. |
| [`crates/ferric-cli/src/server.rs`](../../../../crates/ferric-cli/src/server.rs) | Lifecycle coordinator and consumers | `up`, `status`, `adopt`, `down`, doctor, and managed discovery contain the intended safety order but directly bind real I/O. A post-bind `try_wait` error can orphan a child; listener-check failure can skip reaping; wildcard teardown contradicts E02-B; status cannot enumerate dropped absent scopes; doctor performs unrelated probes before early blocks. |
| [`crates/ferric-cli/src/backend.rs`](../../../../crates/ferric-cli/src/backend.rs) | E03-C backend policy | Automatic discovery uses the common reader and fails closed. The return type is still `Result<Option<ServerRunfile>, String>`, so endpoint source and blocked-state classes are erased; explicit/configured endpoints bypass inventory by policy without a typed marker. |
| [`crates/ferric-cli/src/autonomy_cmd.rs`](../../../../crates/ferric-cli/src/autonomy_cmd.rs) | E03-C strict consumer | Initial managed binding uses shared discovery, but strict mode can probe HTTP before requiring managed state and final validation reuses a cached runfile without re-inventorying new peer conflicts. |
| [`crates/ferric-cli/tests/server_lifecycle_fixture.rs`](../../../../crates/ferric-cli/tests/server_lifecycle_fixture.rs) | E05-C/D black-box proof | Exercises the real CLI and harmless engine fixture. It releases an “unused” port before launch, has a possible 300-second pre-guard stall, is not process-wide serialized, mutates tokens into an invalid shape, and lacks complete status/down/doctor Tailscale assertions. |
| [`crates/ferric-cli/src/bin/ferric_lifecycle_fixture.rs`](../../../../crates/ferric-cli/src/bin/ferric_lifecycle_fixture.rs) | Model-free engine | Correctly enforces loopback and records invocation. It serves connections serially, so one half-open probe can delay readiness. It needs bounded scripted failure modes for binding/publication tests. |
| [`crates/ferric-cli/Cargo.toml`](../../../../crates/ferric-cli/Cargo.toml) | Feature boundary | The fixture binary and integration test require `lifecycle-fixture`; default workspace test commands do not execute them. |
| [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml) | Cross-platform gate | The main matrix covers Ubuntu/Windows but uses default features. No job compiles and runs the lifecycle fixture feature; AArch64 also omits its test surface. |
| [`docs/server-configuration.md`](../../../server-configuration.md) | Operator contract | Already describes exact identity, fail-closed ambiguity, adoption, and blocked Tailscale. Some current implementation behavior (notably wildcard teardown) diverges from the finalized E02-B clause and must be corrected rather than re-documented as accepted. |

### Confirmed implementation defects

1. **Start-token authority is under-validated.** Native code emits
   `windows-filetime:<u64>` or
   `linux-boot-id:<uuid>;start-ticks:<u64>`, but schema v2 accepts arbitrary
   nonblank text. Malformed or foreign tokens can reach resolution instead of
   being an invalid-schema blocker.
2. **Wildcard teardown violates E02-B.** `OwnedByTargetWildcard` currently
   authorizes `down`, and an existing test requires successful termination.
   The finalized clause allows destructive action only for exact loopback
   ownership or an absent listener.
3. **One post-bind failure can orphan the child.** After `acquire_child`
   succeeds, an error from the immediate `Child::try_wait` returns without
   terminating/waiting through the retained process object or publishing a
   recovery record.
4. **A known-exited child can skip reaping.** `stop_managed_child` checks
   listener release before calling `child.wait`; a foreign or uninspectable
   remaining listener returns early and skips the explicit reap.
5. **Linux argv identity is lossy.** `/proc/<pid>/cmdline` uses
   `to_string_lossy`, so distinct non-UTF-8 byte strings can collapse to the
   same authority. Unsupported bytes must make inspection unverifiable unless
   a future schema stores exact bytes.
6. **Linux shared-listener visibility is incomplete.** The adapter skips an
   unreadable peer `/proc/<pid>/fd`. If that peer inherited the target socket,
   the already-accounted inode can still be reported as exclusive. The sprint
   must at least make all observable/induced uncertainty fail closed and record
   the kernel-visibility limitation rather than claiming impossible proof.
7. **Scope/origin state and block type are lost.** Lifecycle expansion drops
   absent local/global/origin slots, aliases an origin to an already captured
   local path instead of independently recording its observation, and converts
   a changed-but-parseable origin into a captureless string blocker that loses
   its raw bytes. The resolver then collapses conflict and unverifiable states
   into strings. Status and downstream consumers therefore cannot meet the
   lossless E01/E03 diagnostic contract.
8. **Strict consumers can act before or after stale discovery.** Strict
   autonomy may contact an endpoint before it requires managed state, then
   revalidates a cached runfile without discovering a newly introduced peer.
   Doctor also performs binary/model probes before a Tailscale or registration
   block is rendered.
9. **Publication and teardown faults are not deterministically driveable.** The
   real code handles many errors conservatively, but terminate, wait, listener
   transition, directory-sync, child-exit-during-publication, per-alias cleanup,
   and rollback failures cannot be exhaustively induced through current APIs.
10. **The black-box gate is absent from CI and can flake.** The three
    feature-gated lifecycle tests do not run under `cargo test --workspace`;
    port release/rebind, long readiness timeout, and serial HTTP handling add
    avoidable nondeterminism.

## 3. External Sources

- [Rust `std::fs` — Time of Check to Time of Use](https://doc.rust-lang.org/std/fs/index.html#time-of-check-to-time-of-use-toctou)
   explicitly recommends atomic operations such as create-new and warns that
   metadata checks and symlink state can change between check and use. This
   supports preserving the current same-directory no-clobber/rename design and
   testing atomic outcomes instead of replacing it with check-then-delete code.
- [Linux `pidfd_open(2)`](https://www.man7.org/linux/man-pages/man2/pidfd_open.2.html)
   defines a pidfd as a descriptor referring to a task and documents using it
   with `pidfd_send_signal` and `poll`. This supports retaining the existing
   pidfd adapter and placing test seams above it; numeric PID signaling must not
   re-enter the lifecycle controller.
- [Microsoft `GetProcessTimes`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)
   specifies creation time as a 64-bit FILETIME count. This supports the stable
   `windows-filetime:<u64>` authority tag and strict canonical decimal parsing.
- [GitHub Actions — Running variations of jobs in a workflow](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/run-job-variations)
   documents OS matrix jobs. The lifecycle feature gate should run as an
   explicit Ubuntu/Windows matrix with serialization and a bounded job timeout,
   not be assumed to follow from default-feature workspace tests.

## 4. Risks, Unknowns, Dependencies

- **Do not replace native ownership with mocks.** Deterministic fakes prove the
  state machine; platform smoke tests must still prove real HANDLE/pidfd token,
  listener, terminate, wait, and release behavior on their native CI runner.
- **Linux exclusive socket ownership has a visibility ceiling.** `/proc` access
  can be restricted, and inherited/SCM-transferred sockets can share one inode.
  The controller must treat an `Uninspectable` result as non-authorizing. Any
  native adapter change must avoid making ordinary unprivileged lifecycle use
  impossible while never describing incomplete enumeration as exhaustive.
- **Strict start-token validation changes old test data.** Existing helpers and
  stale-generation tests use `token`, `opaque`, or append `-stale`. They must
  use valid same-OS alternative tokens when testing PID reuse and separate
  malformed-token cases when testing schema rejection.
- **Store injection must preserve production atomicity.** Introduce a narrow
  operation adapter or scripted callback boundary around existing primitives;
  do not reimplement the filesystem algorithm in a fake or weaken no-clobber,
  file-sync, Unix parent-sync, exact-byte capture, or recovery-path retention.
- **Output capture should follow structured results.** Trying to intercept
  scattered `println!` calls would create brittle tests. Commands should render
  typed discovery/down/up reports while unit tests assert the report and
  black-box tests assert the final copy/paste output.
- **Feature CI time is bounded.** Use a dedicated two-OS lifecycle job with
  `--test-threads=1`, explicit timeout, and the feature enabled. The ordinary
  workspace matrix remains useful and should not be made dependent on a model.
- **Task boundary:** T-11510 still owns positive Tailscale lifecycle support.
  Sprint 117 proves refusal and preservation only. T-11505 through T-11509 stay
  blocked until T-11606 is accepted.

## 5. Recommended Approach

1. **Freeze the acceptance ledger before Build.** Copy the nineteen finalized
   EARS identifiers and exact executable names into the Sprint 117 Test plan.
   Give every clause a concrete test command, test layer, and expected
   side-effect ledger. No aggregate count may stand in for a named proof.
2. **Repair schema and typed discovery first.** Add strict current-OS token
   parsing; retain every configured scope/origin state; split conflict from
   unverifiable resolution; introduce `ManagedServerDiscovery` with `Empty`,
   `Ready`, `Degraded`, `StaleOnly`, and typed `Blocked` variants. Preserve
   aliases, stale registrations, scope, path, identity, listener, health, and a
   stable registration key. Add a small `EndpointSelection` wrapper for
   explicit/managed/default policy.
3. **Introduce narrow internal side-effect seams.** Define retained-process,
   process-runtime, listener-inspector, spawned-child, health-probe, and
   registration-store interfaces. They must expose opaque retained generations
   and never a PID-accepting signal method. Route real `up`, `down`, adoption,
   discovery, and cleanup through typed reports. Script fakes with event ledgers
   drive PID remap, listener transitions, terminate/wait faults, publication
   stages, concurrent replacement, and per-alias cleanup without changing the
   native adapters.
4. **Fix known lifecycle divergences while routing through the seams.** Reject
   malformed/foreign tokens, block wildcard teardown, fail Linux non-UTF-8 argv
   closed, terminate or preserve recovery evidence on every post-bind failure,
   always reap a known-exited child, put Tailscale/typed-registration blocks
   before unrelated doctor/strict-autonomy effects, and rediscover the managed
   registration at strict revalidation.
5. **Build the matrices from pure to native.** Complete E01 store/schema and
   E03 resolver/consumer tables first; then E02/E04 retained-handle and
   transition tables; then E05 publication/compensation tables. Add a real
   two-process shared-path race, native Windows/Linux harmless-child smokes,
   and black-box status/adoption/Tailscale contracts. Real filesystem races
   must include two workspaces publishing distinct records to one shared global
   path, publisher-versus-remover replacement, and two removers yielding one
   removal plus one absence. Each finalized name must
   be present in `cargo test -- --list` and executed individually or by a
   recorded containing command.
6. **Harden the model-free fixture and CI gate.** Serialize lifecycle tests,
   start cleanup guards before blocking CLI calls, add bounded subprocess
   watchdogs, retry only an explicitly diagnosed address-in-use launch, handle
   fixture connections independently, and use valid alternative OS tokens.
   Add a dedicated Ubuntu/Windows job that clippies the fixture surface and runs
   the integration test with `--features lifecycle-fixture --locked --
   --test-threads=1`; compile-check the feature for AArch64 where runnable
   native behavior is unavailable.
7. **Close only from immutable evidence.** Record formatter/linter/unit/
   integration/native/fixture commands and outputs, immutable commit heads,
   two-OS CI run URLs, and an accepted independent critic verdict. If any
   finalized clause remains unmapped or CI is unavailable, fail the sprint
   closed again rather than advancing the wider local-model backlog.

## Artifacts

- Sprint initialization baseline: branch `dev`; local, `origin/main`, and
  `origin/dev` all at `baebc04686e04df510b8127970d6c19eb30145e0`.
- Prior acceptance contract: [Sprint 116 Build plan](../../s116/sprint-plans/build-plan.md)
  and [Test plan](../../s116/sprint-plans/test-plan.md).
- Failed-close input: [Sprint 116 failure report](../../s116/failure-report.md)
  and [test critique](../../s116/sprint-tests/critique.md).
- Research method: direct inspection of eighteen repository files, four
  primary external sources, exact-name source search, and three independent
  read-only audits covering registration/resolution, process/down, and
  publication/fixture/consumer behavior.
- Research budget: 18 repository files and 4 external URLs; within the default
  maxima of 20 files and 5 URLs.
