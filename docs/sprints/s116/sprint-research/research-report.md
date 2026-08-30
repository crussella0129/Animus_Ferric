# Sprint 116 Research Report

## Intents Reviewed

- [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md)
  was **selected** for Sprint 116's lifecycle work. During Build, the user
  explicitly directed that the external refactor report's durable product
  outcomes also be promoted into intents, so INT-0008 was revised to make the
  compact idempotent front door, setup flow, aliases, and advanced-command
  compatibility explicit. Sprint 116 itself advances only exact-process
  ownership, stale-state handling, per-file atomic state publication, and
  cleanup boundaries; it does not claim the compact front door complete.
- [INT-0007 — Hardware-calibrated autonomous development](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
  was revised during Build by the same explicit direction. It now owns the
  report-derived accelerated-backend, speed-aware timeout, reasoning/action
  budget, modern context/profile, first-run calibration, and tuned-compaction
  outcomes. The external report remains directional evidence, not completion
  proof for those criteria.

## 1. Sprint Goal

Replace Ferric's lossy local-first server registration lookup and PID-only
teardown with a two-scope registration inventory and identity-bound lifecycle.
`status` must show both local and global state without allowing a stale local
record to shadow one uniquely verified live server. `down` must acquire and
retain an exact process handle, bind creation identity, executable, argv, and
loopback-listener state, terminate only through that handle, and remove only
the registration bytes captured by the same operation when those bytes remain
unchanged. New registrations must be published without exposing partial JSON
or overwriting state that appeared after preflight. Cross-scope operations are
not linearizable: safety comes from per-path no-clobber publication, typed
split observations, retained process handles, ordered compensation, and
atomic conditional removal.

This is the safety prerequisite for the smaller human command surface in
INT-0008. It does not add the front door, benchmark calibration, model
execution, runtime acquisition, or a new application trial.

## 2. Existing Code Survey

| Project file | Research finding |
| --- | --- |
| `crates/ferric-cli/src/server.rs` | `read_runfile_impl` returns the first parseable local record and otherwise falls through to global state, so it cannot represent two records, malformed state, read failure, or a split identity. |
| `crates/ferric-cli/src/server.rs` | `status` currently combines numeric-PID liveness with an HTTP 200 response. It does not prove process creation, executable, argv, or listener ownership. |
| `crates/ferric-cli/src/server.rs` | `down` signals by numeric PID, ignores the signal result, rechecks the number, and then removes newly resolved local and global paths. PID reuse, split mirrors, and a concurrent registration rewrite are not bound to the destructive action. |
| `crates/ferric-cli/src/server.rs` | `up` writes local and global JSON independently with ordinary `std::fs::write` and succeeds when either write succeeds. A short write, second-scope failure, or registration appearing after preflight can leave partial or split state. |
| `crates/ferric-cli/src/server.rs` | Windows and Linux listener inspection filters by port and state but does not first require an exact loopback address. Linux's parser also proves only whether the target PID owns a matching socket inode, not whether another process owns the expected loopback listener. |
| `crates/ferric-cli/src/server.rs` tests | Current coverage proves launch preflight, HTTP readiness, one happy PID/listener inspection, runfile compatibility, and numeric process liveness. It has no `status`/`down`, cross-workspace, malformed/split, PID-reuse, retained-handle, partial-publication, or compare-before-delete regression. |
| `crates/ferric-cli/tests/cli.rs` | The black-box suite contains no server lifecycle case. Child-scoped `APPDATA` can isolate the global registration without mutating process-wide environment state. |
| `crates/ferric-cli/src/config.rs` | Global server state is derived beside the user configuration path. The injectable path resolver and child-scoped environment precedent allow deterministic local/global fixtures without touching the operator's real registration. |
| `docs/intents/INT-0008-unified-local-model-workflow.md` | Requires exact ownership, stale-state recovery, atomic publication, truthful status, and scoped idempotent cleanup before a compact workflow may compose these primitives. |
| `docs/sprints/s115/sprint-research/external-field-report-adjudication.md` | Confirms the stale-local/live-global failure mechanism and ranks it before command-surface simplification. It treats the supplied field report as external evidence rather than implementation authority. |
| `docs/sprints/s115/sprint-tests/test-report.md` | Sprint 115 ended process-cold after an exact-hash stale-local cleanup, but the frozen app trial did not run. Sprint 116 must not reinterpret that closeout as product lifecycle coverage. |
| `docs/sprints/s115/control-artifacts/runtime/README.md` and `runtime-common.ps1` | Provide a proven Windows safety pattern: revalidate exact creation/executable/argv/runfiles/listener, acquire a durable process handle, terminate and wait through it, then compare retained runfile hashes before deletion. That sprint-specific PowerShell control is evidence and design input, not code to transplant. |

The bounded product design is an additive runfile schema v2. A v2 record has a
required process identity containing PID, a tagged opaque OS start token,
executable, and observed argv, plus an absolute originating local-registration
path. That path lets a global record observed from workspace A name and clean
the matching local alias in workspace B. A record without a version/identity
remains a readable legacy v1 record, but a live v1 process cannot authorize
destructive teardown. A claimed v2 record with missing identity or origin is
malformed rather than silently downgraded. Recovery from a live v1 record
requires an explicit, non-destructive adoption operation that proves the live
process through a retained handle and conditionally replaces only the
unchanged v1 bytes with a v2 identity record.

Inventory observes local and global paths independently as `Absent`,
`Unreadable`, `Malformed`, or captured raw bytes plus a parsed record. A valid
global v2 origin path is then captured independently before it becomes an alias
or cleanup candidate. Live resolution groups records by a verified process key
rather than by scope.
Exact or semantically matching mirrors become aliases of one process while
retaining a separate captured path and byte token for cleanup. More than one
distinct verified live process is a conflict. The same process identity with
conflicting canonical runfile metadata is also a blocker rather than an alias.
A malformed, unreadable, or live legacy candidate is unverifiable and blocks
destructive action.

Listener inspection needs a three-way result. An exact loopback listener owned
by the retained target and an absent listener both allow identity-bound
teardown; absence is necessary so a hung server that dropped its socket can
still be stopped. A listener owned by another process, multiple owners, or an
uninspectable listener blocks teardown. HTTP health affects `status` but never
authorizes or forbids teardown.

The critical cross-workspace resolution is explicit: stale local A plus one
verified live global B resolves to B. `down` terminates B through B's retained
handle, confirms exit and listener release, then reports B stopped and A
stale-cleaned as distinct outcomes. Two live identities never select one by
precedence.

## 3. External Sources

- The operator-supplied field report, retained by SHA-256 and adjudicated in
  [Sprint 115](../../s115/sprint-research/external-field-report-adjudication.md),
  is the external observation that reproduced stale-local shadowing and an
  orphaned global server. Its recommendations are not instructions; every
  Sprint 116 requirement is independently bounded by code and intent.
- Microsoft documents that [`OpenProcess`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess)
  returns a process-object handle usable by wait functions and that
  [`GetProcessTimes`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)
  obtains creation identity from that handle. Windows teardown should validate
  and terminate the same retained object rather than invoke `taskkill` with a
  later numeric lookup.
- Linux [`pidfd_open(2)`](https://www.man7.org/linux/man-pages/man2/pidfd_open.2.html)
  provides a pollable stable process reference, and
  [`pidfd_send_signal(2)`](https://www.man7.org/linux/man-pages/man2/pidfd_send_signal.2.html)
  explicitly avoids the PID-recycling race of traditional `kill(2)`.
- Rust's [`OpenOptions::create_new`](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new)
  is an atomic no-clobber create primitive. It closes the current
  preflight-exists-to-write-overwrite window for same-directory staging names.
- The pinned `tempfile` API documents
  [`NamedTempFile::persist_noclobber`](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist_noclobber)
  as failing when the destination already exists. It may leave both temporary
  and final hard links if interrupted during persistence, so Ferric must treat
  cleanup failure as a reported partial outcome rather than hide a stage.
- Rust's [`std::fs::rename`](https://doc.rust-lang.org/std/fs/fn.rename.html)
  can replace an existing destination and has platform-specific behavior, so a
  plain rename is not itself the required cross-platform no-clobber commit or
  compare-before-delete primitive.
- Tailscale's current [`serve` CLI reference](https://tailscale.com/docs/reference/tailscale-cli/serve)
  documents endpoint-scoped `off` commands that must repeat the original Serve
  flags. That reinforces the requirement to capture exact pre/post Serve state
  before Ferric can safely restore this mode; a broad reset is not a substitute
  for owned endpoint cleanup.

## 4. Risks, Unknowns, Dependencies

- **PID reuse and adapter safety:** process identity must survive the gap
  between inspection and signal. Windows needs an owned process handle;
  supported Linux needs a pidfd. Numeric relookup after validation reopens the
  defect. Launch has an earlier version of the same race: bind Windows directly
  from the spawned `Child` process object; on Linux open the pidfd immediately
  and require the original child still running before readiness. Other
  platforms have no destructive lifecycle authority in this sprint; the
  implementation keeps them compiling through an explicit adapter that
  returns an unsupported, fail-closed result.
- **Creation-token portability:** Windows should retain an exact FILETIME-based
  token from the process handle. Linux needs boot identity plus `/proc` start
  ticks so a reboot cannot make a repeated PID/start-tick pair look identical.
  Human-readable timestamps are diagnostics, not equality keys.
- **Listener classification:** a healthy endpoint can belong to the wrong
  process. Ferric pins managed serving to the exact registered IPv4 loopback
  address `127.0.0.1`; Windows ownership inspection is scoped to that endpoint,
  while Linux also inspects IPv6 tables to reject wildcard/dual-stack
  ambiguity. Address, port, state, and ownership are separate from HTTP. A
  foreign or ambiguous owner blocks signal.
- **Legacy compatibility:** v1 remains readable for status and dead stale-file
  cleanup. A live v1 record cannot authorize a kill because it lacks creation
  identity. The diagnostic must give a copy/paste-complete explicit adoption
  path; adoption acquires and validates the retained target before a
  compare-and-replace upgrade. Silent best-effort PID teardown is forbidden.
- **Inventory ambiguity:** malformed/unreadable state may be the only record of
  another live process. It must remain visible and block destructive selection,
  even when the other scope names a healthy server.
- **Stale listener reconciliation:** an absent or reused registered PID does
  not prove its port is unused. Stale cleanup is allowed only when the exact
  registered listener is absent or its owner is accounted for by the uniquely
  selected same-port process; foreign or uninspectable ownership preserves the
  record and blocks mutation.
- **Endpoint self-consistency:** process/listener authority uses the registered
  port while backend consumers use `base_url`. A non-Tailscale v2 record is
  authoritative only when its URL is exactly the managed
  `http://127.0.0.1:<port>/v1` endpoint and its executable path is absolute;
  otherwise one record could validate one endpoint and route work to another.
- **Two-scope publication:** local and global files cannot be committed in one
  filesystem transaction. Serialize once, exclusively stage and file-sync
  complete bytes in each configured directory, persist each stage without
  clobbering an existing final name, and sync the parent directory where
  supported. On
  second-scope failure, stop/wait the still-held child and compare-rollback
  only the unchanged first publication. This is per-file atomic publication
  with compensation, not a transaction across both scopes.
- **Rollback exit proof:** publication compensation must not erase the only
  clue to a child that is still alive. Registration rollback begins only after
  terminate-and-wait proves the retained child generation exited; signal/wait
  failure keeps published registrations and reports recovery state.
- **Durable proxy state:** current `--tailscale` setup mutates node-wide Serve
  configuration before publication and has no Ferric-owned token for exact
  cleanup or compensation. A blind reset could remove unrelated operator
  state. Sprint 116 must refuse this mode before any side effect and treat any
  captured `tailscale: true` registration as non-destructively blocked before
  PID inspection, even when the PID is absent; T-11510 will own a future exact
  capture/compare/restore protocol.
- **Compare-before-delete:** cleanup is per captured path and exact raw bytes,
  not parsed-object equality or reserialization. Atomically isolate the current
  named entry in a same-parent holding location before reading it. A later
  replacement at the original name is
  never opened or removed. Changed/unreadable state is restored without
  clobbering or preserved at a reported holding path; non-`AlreadyExists`
  restore failures are errors, not ordinary replacement outcomes. Tests need
  isolation/replacement/restore seams and two-process per-path interleavings.
- **Consumer compatibility:** backend discovery, strict autonomy preflight,
  doctor, status, and down currently consume the lossy helper. The new typed
  resolver must not make ambiguity look like absence or silently fall back to
  the built-in endpoint.
- **Scope control:** T-11505 through T-11509, Qwen execution, model/runtime
  download, benchmark limits, reasoning support, context inheritance, and the
  compact front door remain ordered follow-up. No live model is needed for
  Sprint 116's deterministic acceptance suite.

## 5. Recommended Approach

1. Introduce schema-v2 process identity and a lossless two-scope inventory.
   Require an absolute originating-local coordinate so a global capture can
   identify its local alias. Preserve legacy parsing while making live v1 and
   malformed claimed-v2 state non-authorizing.
2. Keep cross-scope operations explicitly non-linearizable. Compose per-path
   no-clobber publication, typed split observations, ordered exact
   compensation, retained process handles, and atomic conditional removal so
   concurrent invocations cannot overwrite or delete another operation's state.
3. Separate pure lifecycle resolution from platform adapters. The resolver
   consumes captured registrations, retained-process facts, three-way
   listener state, and health facts and returns typed `none`, `one`,
   `conflict`, `unverifiable`, stale, and alias outcomes.
4. Implement Windows retained-handle and Linux pidfd adapters. Acquire the
   handle first, then validate the same object's creation token, executable,
   argv, and listener state. Bind launched children immediately from the Child
   process object on Windows or from an immediate pidfd plus original-child
   liveness check on Linux, before readiness polling. Keep other platforms
   outside destructive lifecycle authority; any compile-only fallback must
   return an explicit unsupported result and fail closed.
5. Make `status` enumerate both scopes and report the aggregate result. Make
   `down` terminate only one uniquely verified process through the retained
   handle, wait on that handle, verify required listener release, and then
   compare-delete unchanged captured registrations. Never print `stopped` for
   stale cleanup or failed ownership.
6. Add explicit `server adopt --pid <pid>` recovery for live v1 records. It
   must validate the retained process, engine contract, available argv facts,
   and listener state before conditionally replacing unchanged v1 bytes; it
   never signals the target.
7. Serialize the v2 record once, exclusively stage and sync complete bytes in
   each destination directory, and atomically persist each stage without
   clobbering the final name. Configured global publication failure is launch
   failure with exact
   compensating rollback; an unavailable global path legitimately permits
   local-only publication.
8. Build the acceptance suite entirely from temp directories, fake
   process/listener/store adapters, loopback fixtures, and harmless child
   processes. Include the stale-A/live-B reproduction, split live identities,
   PID reuse before and after handle acquisition, listener disagreement,
   concurrent per-path interleavings, partial publication, and
   compare-before-delete mutation.
   For CLI lifecycle E2E, build a separate feature-gated fixture executable,
   expose it only as `llama-server(.exe)` in an isolated child `PATH`, and use
   a dummy regular model plus loopback health response; do not add a production
   test hook.
9. Refuse `server up --tailscale` before any process, CLI, or filesystem side
   effect and report it blocked in doctor. Treat every captured
   `tailscale: true` record as a lifecycle blocker before PID inspection until
   T-11510 supplies exact proxy-state ownership and compensation.
10. Run formatting, clippy, targeted/default/full tests, platform-conditional
   adapter tests, and a no-model cross-workspace CLI lifecycle. Record only
   observed results in the Test phase.

## Artifacts

- [Sprint 115 field-report adjudication](../../s115/sprint-research/external-field-report-adjudication.md)
- [Sprint 115 partial test report](../../s115/sprint-tests/test-report.md)
- [INT-0008 unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md)
- [Active and ordered work ledger](../../../work/tasks.md)
- [Current server lifecycle implementation](../../../../crates/ferric-cli/src/server.rs)
- [Current CLI integration tests](../../../../crates/ferric-cli/tests/cli.rs)

The research audit inspected 19 relevant project files spanning current
server/config/test code, Sprint 114/115 Book formats and lifecycle evidence,
the active intent/work/navigation ledgers, and the supplied external report's
tracked adjudication. It used five external source groups and stayed within the
phase budgets; no budget override is required.
