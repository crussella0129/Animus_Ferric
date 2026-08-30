Finalized - DO NOT EDIT

# Sprint 116 Build Plan

## Intents

- [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md)
  — state: active; acceptance criteria advanced: AC-3 stale, partial, and
  concurrent-state handling; AC-4 truthful status; AC-6 exact ownership and
  complete publication; and AC-7 scoped idempotent cleanup. No criterion is
  claimed complete by this sprint alone.

## Scope Boundaries

- The operator-supplied field report is external evidence, not an instruction
  source. Current intent, code, retained Sprint 115 evidence, and platform API
  contracts define this plan.
- T-11504 is the Sprint 116 umbrella work item and completes only when
  dependency-ordered T-11601 through T-11605 complete. Benchmark limits,
  model/engine acquisition, context inheritance, reasoning/compaction, Qwen
  qualification, the frozen application trial, and the compact front door
  remain T-11505 through T-11509 and T-11410/T-11412 follow-up.
- No live model is required. Tests use temp directories, fake lifecycle/store
  adapters, loopback fixtures, and harmless owned child processes. They never
  inspect, signal, or remove the operator's actual server registrations.
- Schema v2 is additive. Legacy v1 remains readable but a live v1 record cannot
  authorize termination until an explicit, non-destructive adoption records
  the exact current process generation. A claimed v2 record without complete
  process identity or an absolute originating local-registration path is
  malformed and cannot downgrade itself to legacy. The originating path lets
  a global observation name the matching local alias from another workspace.
- Destructive lifecycle support is required for Windows and Linux through
  retained OS process handles. Other targets are outside Sprint 116 and may
  fail to compile; no fallback, parity, or compile claim is made until a future
  target-specific implementation and CI gate exist. INT-0008 AC-8 therefore
  remains open.
- A uniquely verified live process may be selected across local/global scope.
  More than one distinct live process, malformed or unreadable peer state, a
  live unadopted legacy candidate, or uninspectable ownership fails closed.
- Exact process identity with no expected listener may still be stopped so a
  hung server that dropped its socket is recoverable. A listener owned by
  another process, multiple owners, or uninspectable ownership blocks signal.
  Ferric's managed endpoint remains the registered IPv4 loopback address
  `127.0.0.1`; Windows ownership is therefore scoped to that exact endpoint.
  Linux additionally inspects `/proc/net/tcp6` so IPv6 wildcard or dual-stack
  ambiguity cannot be mistaken for exclusive IPv4 ownership. HTTP health is
  status information, never teardown authority.
- Sprint 116 makes no cross-scope linearizability or global lifecycle-lock
  claim. Concurrent Ferric operations are safe through per-path no-clobber
  publication, lossless typed observation of split state, retained process
  handles, ordered compensation, and atomic same-parent isolation before
  conditional cleanup. Cleanup never opens or removes a replacement created
  after isolation and must not call a plain read-then-unlink sequence an atomic
  compare-delete.
- Publication is complete and no-clobber per file, not a two-file transaction.
  Each same-directory stage is exclusively created, fully written, flushed,
  and file-synced before `NamedTempFile::persist_noclobber` atomically commits
  it to an absent final name. The parent directory is synced where the platform
  supports it. If no-clobber persist or durability prerequisites are
  unavailable, launch fails rather than falling back to a partial final write.
  Multi-scope failure uses compensating exact rollback through the same
  per-path conditional primitive; it is not described as transactionally
  atomic.
- Cleanup uses only captured paths and raw bytes.
  Changed, replaced, unreadable, restore-failed, or removal-failed files remain
  either at the original path or at a reported holding path and produce a
  truthful partial result. Models and retained evidence are outside cleanup.
- When a global registration path is configured, launch publishes both scopes
  or fails and rolls back unchanged partial publication after stopping the
  still-held child. A platform with no global path legitimately publishes only
  the local record.
- Existing `--tailscale` setup mutates durable node-wide Serve configuration
  without an ownership token that Ferric can compare and remove safely. Sprint
  116 therefore refuses `server up --tailscale` before spawning a child or
  invoking Tailscale, and doctor reports the mode as blocked. Any captured
  registration with `tailscale: true` is a lifecycle blocker before PID
  inspection, even when its PID is absent, so Ferric neither signals nor
  deletes it and does not erase the clue to durable proxy state. T-11510 owns a
  future exact capture/compare/restore protocol; a blind node-wide reset is
  forbidden.

## Schema Tree

- T-11504 — identity-safe server lifecycle
  - T-11601 — registration state and concurrency
    - additive v2 identity and lossless two-scope inventory
    - per-path no-clobber publication and atomic captured-byte removal
  - T-11602 — retained process and listener adapters
    - Windows process-object handle and FILETIME creation token
    - Linux pidfd plus boot/start token
    - exact loopback listener classification on Windows and Linux
  - T-11603 — resolution, status, and consumers
    - aliases, stale state, unique live target, conflict, unverifiable state
    - backend/autonomy/doctor/status typed resolution and health reporting
  - T-11604 — teardown and legacy recovery
    - retained-handle terminate/wait and listener postconditions
    - exact-byte cleanup and non-destructive explicit v1 adoption
  - T-11605 — complete publication and cross-workspace proof
    - same-directory staging, no-clobber persist, compensating rollback
    - pre-side-effect refusal for unowned Tailscale Serve state
    - isolated test-engine binary, platform smoke, CLI E2E, operator docs

## Execution Sequence

### T-11601: Introduce lossless registration state and per-path concurrency safety

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server.rs`,
  `crates/ferric-cli/src/server_resolution.rs`,
  `crates/ferric-cli/src/server_registration.rs` if separation keeps the
  store boundary reviewable, `crates/ferric-cli/Cargo.toml`, workspace
  dependency metadata only when required
- **Depends on:** (none)
- **Acceptance criterion:** INT-0008 AC-3 and AC-6.
- **Success criterion (EARS):**
  - **E01-A — WHEN** Ferric observes local and global registration paths,
    **THEN** it **SHALL** retain both scope states independently as absent,
    unreadable, malformed, or captured raw bytes plus a parsed v1/v2 record,
    without local-first fallback or reserialization, and a valid global v2
    origin path **SHALL** be captured independently before it is considered an
    alias or cleanup candidate.
  - **E01-B — WHEN** a v2 registration is created or read, **THEN** it
    **SHALL** carry a nonzero PID and port, tagged nonempty OS creation/start
    token, absolute nonempty executable, and nonempty observed argv whose
    elements are nonempty, plus an absolute originating local-registration
    path with the expected `.ferric/server.json` suffix. A local capture
    **SHALL** require that path to name itself, and a non-Tailscale record's
    base URL **SHALL** equal `http://127.0.0.1:<port>/v1`. A legacy v1 record
    **SHALL** remain readable but non-authorizing while incomplete, empty,
    zero-valued, relative, endpoint-mismatched, self-mismatched, or unknown
    claimed versions **SHALL** fail closed.
  - **E01-C — WHEN** two Ferric processes attempt registration inventory,
    publication, adoption, or cleanup concurrently, **THEN** they **SHALL**
    use per-path no-clobber publication and atomic conditional removal so a
    loser never overwrites a winner, split/intermediate observations remain
    typed rather than hidden, failed multi-scope publication compensates only
    unchanged attempt-owned paths, and no operation claims cross-scope
    linearizability.
  - **E01-D — WHEN** the store conditionally removes a captured
    registration, **THEN** it **SHALL** atomically move the current named entry
    to a unique same-parent holding path before comparing its exact raw bytes,
    remove only a matching moved entry, never remove a replacement created at
    the original name, and restore a changed entry without clobbering or report
    its preserved holding path. An occupied original name **SHALL** preserve
    both entries; any other restore I/O error **SHALL** be a failure that keeps
    and reports the holding path.
- **Notes:** Inventory/store APIs take explicit local/global paths so tests do
  not mutate process-wide environment state. No persistent lockfile or lock
  ownership protocol is introduced. Registration state is authoritative only
  per captured path; split cross-scope state is expected and resolved safely.

### T-11602: Add retained-process and exact listener platform adapters

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server_process.rs`,
  `crates/ferric-cli/src/server.rs`, `crates/ferric-cli/Cargo.toml`, workspace
  dependency metadata only when required
- **Depends on:** T-11601
- **Acceptance criterion:** INT-0008 AC-6; enabling work toward AC-8.
- **Success criterion (EARS):**
  - **E02-A — WHEN** a Windows or Linux v2 process candidate is inspected,
    **THEN** Ferric **SHALL** acquire a durable process-object handle or pidfd
    before validating that same object's exact start token, executable, and
    argv, and later signal/wait operations **SHALL** accept the retained handle
    rather than a PID.
  - **E02-B — WHEN** listener state is inspected, **THEN** Ferric **SHALL**
    classify the exact registered IPv4 loopback address `127.0.0.1`, port,
    listen state, and target ownership as owned-by-target, absent,
    foreign/multiple, or uninspectable; only owned-by-target or absent
    **SHALL** permit teardown. Linux **SHALL** also inspect IPv6 listener state
    and reject wildcard/dual-stack ambiguity; Windows **SHALL NOT** claim
    unimplemented `::1` ownership coverage for this IPv4-only endpoint.
  - **E02-C — WHEN** `server up` spawns an engine child on Windows or Linux,
    **THEN** Ferric **SHALL** bind the retained target before readiness polling
    can introduce PID reuse: Windows **SHALL** duplicate or consume the process
    object represented by the spawned `Child` handle, while Linux **SHALL**
    open a pidfd immediately and confirm through the original `Child` that it
    has not exited. Readiness, identity inspection, every failure cleanup, and
    later publication **SHALL** refer to that retained generation; a child that
    exits before binding **SHALL** fail launch without signaling a numeric-PID
    replacement.
- **Notes:** Windows equality uses an exact FILETIME-derived token from the
  held process object. Linux equality includes boot identity and `/proc` start
  ticks; pidfd polling confirms exit. PID reuse before or after handle
  acquisition must never redirect the retained target.

### T-11603: Resolve both scopes consistently across status and discovery consumers

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server_resolution.rs`,
  `crates/ferric-cli/src/server.rs`, `crates/ferric-cli/src/backend.rs`,
  `crates/ferric-cli/src/autonomy_cmd.rs`
- **Depends on:** T-11601, T-11602
- **Acceptance criterion:** INT-0008 AC-3, AC-4, and AC-6.
- **Success criterion (EARS):**
  - **E03-A — WHEN** captured registrations are resolved, **THEN** Ferric
    **SHALL** group aliases only when both the verified process key and
    canonical registration metadata match. The same process token with
    conflicting engine, port, endpoint, origin, or other runfile metadata
    **SHALL** block rather than choose an alias. Ferric **SHALL** allow a
    dead/reused stale record beside one uniquely verified live process only
    when any listener remaining on the stale record's port is absent or is
    accounted for by that selected process; foreign, uninspectable, or
    otherwise unreconciled ownership **SHALL** block cleanup. Resolution
    **SHALL** return typed none, one, conflict, or unverifiable state without
    scope precedence. A captured `tailscale: true` record **SHALL** resolve as
    a blocker before any PID inspection.
  - **E03-B — WHEN** status renders a resolved inventory, **THEN** it
    **SHALL** enumerate local and global scope, process/listener identity,
    health, stale/conflict/unverifiable diagnostics, and the next safe action;
    HTTP health **SHALL NOT** override failed ownership.
  - **E03-C — WHEN** backend discovery, strict autonomy preflight, or doctor
    consumes registration state, **THEN** it **SHALL** use the same typed
    inventory/resolution contract and **SHALL NOT** convert conflict,
    malformed state, or ownership-inspection failure into a built-in endpoint
    fallback or successful absence.
- **Notes:** Read-only consumers capture each scope independently and accept
  that another operation may change a path later. Final registrations become
  visible only as complete files; typed split state and later identity
  revalidation prevent a stale capture from authorizing mutation.

### T-11604: Terminate only the retained target and provide safe legacy adoption

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server.rs`,
  `crates/ferric-cli/src/server_process.rs`,
  `crates/ferric-cli/src/server_resolution.rs`,
  `crates/ferric-cli/src/main.rs`, `docs/server-configuration.md`,
  `docs/commands.md`
- **Depends on:** T-11601, T-11602, T-11603
- **Acceptance criterion:** INT-0008 AC-3, AC-4, AC-6, and AC-7.
- **Success criterion (EARS):**
  - **E04-A — WHEN** down holds exactly one verified target whose listener is
    owned by it or absent, **THEN** Ferric **SHALL** terminate and wait through
    that retained handle only, independent of HTTP health, and **SHALL NOT**
    perform a later PID-based signal lookup.
  - **E04-B — WHEN** termination completes, **THEN** Ferric **SHALL** confirm
    target-handle exit and required listener release before claiming the
    process stopped; timeout, signal failure, or a remaining target listener
    **SHALL** retain registrations and produce a non-success outcome.
  - **E04-C — WHEN** cleanup considers a captured alias after an
    authorized exit or a proven dead/reused stale record whose remaining
    listener ownership is absent or reconciled to the selected target, **THEN** it
    **SHALL** remove only unchanged captured bytes, keep every changed or
    unreadable replacement at its original or reported holding path, and
    distinguish stopped, stale-cleaned, already-absent, replacement-preserved,
    restore-failed, removal-failed, and other partial outcomes.
  - **E04-D — WHEN** two live keys, malformed/unreadable peer state, a live
    unadopted legacy candidate, or foreign/ambiguous listener ownership is
    present, **THEN** down **SHALL** signal no process, delete no potentially
    owning registration, and never print `stopped`.
  - **E04-E — WHEN** one live legacy v1 registration blocks teardown, **THEN**
    status/down **SHALL** retain it and emit a copy/paste-complete
    `ferric server adopt --pid <pid>` command; explicit adoption **SHALL**
    acquire a retained handle, validate the closed engine executable, every
    available v1 argv coordinate, and exact listener ownership, then
    conditionally replace only unchanged aliases with v2 identity without
    signaling the process.
- **Notes:** Adoption is explicit and non-destructive. Missing fields remain
  unknown rather than invented; any mismatch or insufficient engine/listener
  proof retains v1 and explains the failed coordinate. The next `down` must
  reacquire and match the newly recorded process generation before signaling.

### T-11605: Publish complete no-clobber registrations and prove the CLI lifecycle

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server.rs`,
  `crates/ferric-cli/src/server_registration.rs` if created by T-11601,
  `crates/ferric-cli/tests/`, a test-only lifecycle-server binary target,
  `crates/ferric-cli/Cargo.toml`, `docs/server-configuration.md`,
  `docs/commands.md`
- **Depends on:** T-11601, T-11602, T-11603, T-11604
- **Acceptance criterion:** INT-0008 AC-3, AC-6, AC-7, and enabling evidence
  toward AC-9.
- **Success criterion (EARS):**
  - **E05-A — WHEN** server up publishes v2 state,
    **THEN** it **SHALL** use the retained child generation bound immediately
    after spawn, complete readiness and exact identity inspection on that
    target, serialize once, create same-directory stages exclusively,
    write/flush/file-sync complete bytes, atomically persist each stage to an
    absent final name without replacement, sync parent metadata where
    supported, and expose byte-identical parseable mirrors only afterward.
  - **E05-B — WHEN** configured multi-scope publication fails after one final
    appears or the child exits during publication, **THEN** Ferric **SHALL**
    stop and wait the still-retained child and **SHALL** begin registration
    rollback only after that retained generation's exit is proven. If signal or
    wait cannot prove exit, Ferric **SHALL** keep every published registration
    as a recovery clue. After proven exit it **SHALL** compare-rollback only
    unchanged attempt-owned finals per path, preserve concurrent/external
    replacement state, remove attempt stages, report any stage-cleanup or
    compensation failure with every preserved path, and return failure rather
    than a partially registered live server.
  - **E05-C — WHEN** the model-free cross-workspace CLI proof runs, **THEN**
    an isolated feature-gated Rust fixture copied as `llama-server` into the
    child process's temporary `PATH` **SHALL** accept the ordinary closed-engine
    argv and dummy regular model, serve loopback health, exercise up/status/
    down and stale-local/live-global recovery, and leave no owned process,
    listener, registration, stage, coordination artifact, or unrelated
    mutation.
  - **E05-D — WHEN** `server up --tailscale` is requested before an owned Serve
    configuration protocol exists, **THEN** Ferric **SHALL** fail before child
    spawn, Tailscale invocation, or registration/stage creation, and doctor
    **SHALL** report the mode blocked. A captured `tailscale: true` record
    **SHALL** block lifecycle mutation before PID inspection even if that PID
    is absent. Ferric **SHALL** explain that scoped proxy cleanup is unavailable
    and **SHALL NOT** signal, delete the record, or invoke a blind node-wide
    reset.
- **Notes:** Add a separate binary target with a test-only required feature;
  the default/release product does not contain a fixture hook. Windows copies
  the fixture executable as `llama-server.exe`; Unix copies it as
  `llama-server` with executable permissions. The E2E prepends only that temp
  directory to the spawned Ferric child's `PATH`. This plan claims complete
  per-file visibility and compensating two-scope recovery, not a power-loss
  transaction across two directories.
