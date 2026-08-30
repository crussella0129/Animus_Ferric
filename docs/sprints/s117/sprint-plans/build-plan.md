Finalized - DO NOT EDIT

# Sprint 117 Build Plan

## Intents

- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — state: active; acceptance criteria covered: AC-3 explicit partial/stale state, AC-4 truthful status, AC-6 exact process/listener ownership, and AC-7 bounded cleanup. T-11606 is the governing work item.

## Sprint Goal

Turn the partial Sprint 116 identity-safe lifecycle into a clause-provable
cross-platform boundary: lossless typed registration discovery, retained-object
process control, exact cleanup/publication compensation, complete operator
diagnostics, deterministic fault matrices, and a model-free Windows/Linux CI
gate. Positive Tailscale Serve ownership remains outside this sprint.

## Schema Tree

- T-11606 acceptance recovery
  - Registration authority and concurrency
    - T-11701: canonical tokens, lossless coordinates, atomic store matrices
  - Native and scripted runtime ownership
    - T-11702: retained process/child/listener seams and platform corrections
  - Shared discovery and reporting
    - T-11703: typed resolver, status, backend, autonomy, doctor
  - Destructive lifecycle
    - T-11704: retained-handle down, typed cleanup, explicit adoption
  - Launch publication and blocked external state
    - T-11705: complete publication, compensation, early Tailscale refusal
  - Cross-platform acceptance gate
    - T-11706: real races, hardened fixture, native smokes, CI and evidence

## Execution Sequence

### T-11701: Enforce canonical registration authority and lossless per-path state

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server_registration.rs`,
  `crates/ferric-cli/src/server_process.rs`, `crates/ferric-cli/src/server.rs`
- **Depends on:** (none)
- **Acceptance criterion:** INT-0008 AC-3, AC-6, and AC-7; T-11606 E01-A through E01-D.
- **Success criterion (EARS):**
  - **E01-A — WHEN** Ferric observes local and global registration paths,
    **THEN** it **SHALL** retain both scope states independently as absent,
    unreadable, malformed, or captured raw bytes plus a parsed v1/v2 record,
    without local-first fallback or reserialization, and a valid global v2
    origin path **SHALL** be captured independently before it is considered an
    alias or cleanup candidate.
  - **E01-B — WHEN** a v2 registration is created or read, **THEN** it
    **SHALL** carry a nonzero PID and port, a canonical nonempty current-OS
    creation/start token, an absolute nonempty executable, and nonempty
    observed argv whose elements are nonempty, plus an absolute originating
    local-registration path with the expected `.ferric/server.json` suffix. A
    local capture **SHALL** require that path to name itself, and a
    non-Tailscale record's base URL **SHALL** equal
    `http://127.0.0.1:<port>/v1`. A legacy v1 record **SHALL** remain readable
    but non-authorizing while incomplete, empty, zero-valued, relative,
    endpoint-mismatched, self-mismatched, foreign/untagged/noncanonical-token,
    or unknown claimed versions **SHALL** fail closed.
  - **E01-C — WHEN** two Ferric processes attempt registration inventory,
    publication, adoption, or cleanup concurrently, **THEN** they **SHALL** use
    per-path no-clobber publication and atomic conditional removal so a loser
    never overwrites a winner, split/intermediate observations remain typed
    rather than hidden, failed multi-scope publication compensates only
    unchanged attempt-owned paths, and no operation claims cross-scope
    linearizability.
  - **E01-D — WHEN** the store conditionally removes a captured registration,
    **THEN** it **SHALL** atomically move the current named entry to a unique
    same-parent holding path before comparing its exact raw bytes, remove only
    a matching moved entry, never remove a replacement created at the original
    name, and restore a changed entry without clobbering or report its preserved
    holding path. An occupied original name **SHALL** preserve both entries;
    any other restore I/O error **SHALL** be a failure that keeps and reports
    the holding path.
- **Notes:** Keep the serialized token string stable. Centralize strict parsing
  of `windows-filetime:<u64>` and
  `linux-boot-id:<uuid>;start-ticks:<u64>`; test helpers use valid alternate
  same-OS tokens for generation changes. Replace formatted labels with typed
  scope/path/origin coordinates and retain absent, blocked, raw, and parsed
  observations. Preserve the current no-clobber/rename primitives; inject only
  their operation boundary and add real multi-process races in T-11706.

### T-11702: Make retained process, spawned child, and listener transitions deterministic

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server_process.rs`,
  `crates/ferric-cli/src/server.rs`
- **Depends on:** T-11701
- **Acceptance criterion:** INT-0008 AC-6; T-11606 E02-A through E02-C.
- **Success criterion (EARS):**
  - **E02-A — WHEN** a Windows or Linux v2 process candidate is inspected,
    **THEN** Ferric **SHALL** acquire a durable process-object handle or pidfd
    before validating that same object's exact start token, executable, and
    argv, and later signal/wait operations **SHALL** accept the retained handle
    rather than a PID.
  - **E02-B — WHEN** listener state is inspected, **THEN** Ferric **SHALL**
    classify the exact registered IPv4 loopback address `127.0.0.1`, port,
    listen state, and target ownership as owned-by-target, absent,
    foreign/multiple, wildcard/dual-stack, or uninspectable; only exclusive
    owned-by-target or absent **SHALL** permit teardown. Linux **SHALL** also
    inspect IPv6 listener state and reject wildcard/dual-stack ambiguity;
    Windows **SHALL NOT** claim unimplemented `::1` ownership coverage for
    this IPv4-only endpoint. **WHEN** `server up` observes wildcard/public or
    otherwise non-exclusive ownership after binding its child, **THEN** it
    **SHALL** publish no registration, prove that retained child generation
    exited, and report any unproved cleanup as a recovery failure.
  - **E02-C — WHEN** `server up` spawns an engine child on Windows or Linux,
    **THEN** Ferric **SHALL** bind the retained target before readiness polling
    can introduce PID reuse: Windows **SHALL** duplicate or consume the process
    object represented by the spawned `Child` handle, while Linux **SHALL**
    open a pidfd immediately and confirm through the original `Child` that it
    has not exited. Readiness, identity inspection, every failure cleanup, and
    later publication **SHALL** refer to that retained generation; a child that
    exits before binding **SHALL** fail launch without signaling a numeric-PID
    replacement.
- **Notes:** Add internal `RetainedProcess`, `ProcessRuntime`,
  `SpawnedChild`, `ListenerInspector`, and health/clock seams with scripted
  event ledgers; production implementations remain the current HANDLE/pidfd
  adapters. A signal method accepts only the retained object. Fail Linux
  non-UTF-8 argv closed. Never describe incomplete `/proc` shared-owner
  enumeration as exclusive. Correct wildcard teardown, the post-bind
  `try_wait` orphan path, and unconditional reaping of a known-exited child.

### T-11703: Resolve and render one typed lifecycle contract for every consumer

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server_resolution.rs`,
  `crates/ferric-cli/src/server.rs`, `crates/ferric-cli/src/backend.rs`,
  `crates/ferric-cli/src/autonomy_cmd.rs`, `docs/server-configuration.md`
- **Depends on:** T-11701, T-11702
- **Acceptance criterion:** INT-0008 AC-3, AC-4, and AC-6; T-11606 E03-A through E03-C.
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
  - **E03-B — WHEN** status renders a resolved inventory, **THEN** it **SHALL**
    enumerate configured local, global, and promised-origin scope state,
    process/listener identity, health, stale/conflict/unverifiable diagnostics,
    and the next safe action; HTTP health **SHALL NOT** override failed
    ownership.
  - **E03-C — WHEN** backend discovery, strict autonomy preflight, or doctor
    consumes registration state, **THEN** it **SHALL** use the same typed
    inventory/resolution contract and **SHALL NOT** convert conflict,
    malformed state, or ownership-inspection failure into a built-in endpoint
    fallback, successful absence, or a pre-validation external effect.
- **Notes:** Introduce a shared `ManagedServerDiscovery` result with typed
  registration references and issues, plus `EndpointSelection` for explicit,
  managed, and default policy. Keep destructive handles private to lifecycle
  orchestration. Status/doctor render pure reports. Strict autonomy requires a
  ready managed result before HTTP and rediscovers the same registration key at
  final validation; a cached raw runfile cannot bypass a new peer blocker.

### T-11704: Drive teardown and legacy adoption through retained authority and typed cleanup reports

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server.rs`,
  `crates/ferric-cli/src/server_registration.rs`,
  `crates/ferric-cli/src/server_process.rs`,
  `crates/ferric-cli/src/server_resolution.rs`, `docs/server-configuration.md`,
  `docs/commands.md`
- **Depends on:** T-11701, T-11702, T-11703
- **Acceptance criterion:** INT-0008 AC-3, AC-4, AC-6, and AC-7; T-11606 E04-A through E04-E.
- **Success criterion (EARS):**
  - **E04-A — WHEN** down holds exactly one verified target whose listener is
    exclusively owned by it or absent, **THEN** Ferric **SHALL** terminate and
    wait through that retained handle only, independent of HTTP health, and
    **SHALL NOT** perform a later PID-based signal lookup.
  - **E04-B — WHEN** termination completes, **THEN** Ferric **SHALL** confirm
    target-handle exit and required listener release before claiming the
    process stopped; timeout, signal failure, a remaining target listener, or
    uninspectable/foreign ownership **SHALL** retain registrations and produce
    a non-success outcome.
  - **E04-C — WHEN** cleanup considers a captured alias after an authorized
    exit or a proven dead/reused stale record whose remaining listener
    ownership is absent or reconciled to the selected target, **THEN** it
    **SHALL** remove only unchanged captured bytes, keep every changed or
    unreadable replacement at its original or reported holding path, and
    distinguish stopped, stale-cleaned, already-absent,
    replacement-preserved, restore-failed, removal-failed, and other partial
    outcomes.
  - **E04-D — WHEN** two live keys, malformed/unreadable peer state, a live
    unadopted legacy candidate, or foreign/ambiguous listener ownership is
    present, **THEN** down **SHALL** signal no process, delete no potentially
    owning registration, and never print `stopped`.
  - **E04-E — WHEN** one live legacy v1 registration blocks teardown,
    **THEN** status/down **SHALL** retain it and emit a copy/paste-complete
    `ferric server adopt --pid <pid>` command; explicit adoption **SHALL**
    acquire a retained handle, validate the closed engine executable, every
    available v1 argv coordinate, and exact listener ownership, then
    conditionally replace only unchanged aliases with v2 identity without
    signaling the process.
- **Notes:** Make down/adoption return structured reports and let a thin CLI
  adapter print them. The fake runtime/store records acquire, inspect, signal,
  wait, listener, replace, remove, and render order. Adoption rechecks the
  retained generation after conditional replacement and rolls back only its
  unchanged replacements. Every blocker path has an empty signal/delete ledger.

### T-11705: Make launch publication and compensation complete, injectable, and externally bounded

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server_registration.rs`,
  `crates/ferric-cli/src/server.rs`,
  `crates/ferric-cli/src/bin/ferric_lifecycle_fixture.rs`,
  `docs/server-configuration.md`, `docs/commands.md`
- **Depends on:** T-11701, T-11702, T-11703, T-11704
- **Acceptance criterion:** INT-0008 AC-3, AC-6, and AC-7; T-11606 E05-A, E05-B, and E05-D.
- **Success criterion (EARS):**
  - **E05-A — WHEN** server up publishes v2 state, **THEN** it **SHALL** use
    the retained child generation bound immediately after spawn, complete
    readiness and exact identity inspection on that target, serialize once,
    create same-directory stages exclusively, write/flush/file-sync complete
    bytes, atomically persist each stage to an absent final name without
    replacement, sync parent metadata where supported, and expose
    byte-identical parseable mirrors only afterward.
  - **E05-B — WHEN** configured multi-scope publication fails after one final
    appears or the child exits during publication, **THEN** Ferric **SHALL**
    stop and wait the still-retained child and **SHALL** begin registration
    rollback only after that retained generation's exit is proven. If signal
    or wait cannot prove exit, Ferric **SHALL** keep every published
    registration as a recovery clue. After proven exit it **SHALL**
    compare-rollback only unchanged attempt-owned finals per path, preserve
    concurrent/external replacement state, remove attempt stages, report any
    stage-cleanup or compensation failure with every preserved path, and
    return failure rather than a partially registered live server.
  - **E05-D — WHEN** `server up --tailscale` or doctor Tailscale mode is
    requested before an owned Serve configuration protocol exists, **THEN**
    Ferric **SHALL** fail before child spawn, engine version probe, Tailscale
    invocation, PID inspection, or registration/stage creation, and doctor
    **SHALL** report the mode blocked. A captured `tailscale: true` record
    **SHALL** block lifecycle mutation before PID inspection even if that PID
    is absent. Ferric **SHALL** explain that scoped proxy cleanup is unavailable
    and **SHALL NOT** signal, delete the record, or invoke a blind node-wide
    reset.
- **Notes:** Inject scripted persistence stages around the production atomic
  primitive, including precommit and committed-but-durability failures for
  each scope. Compensation consumes the retained process/store interfaces from
  prior tasks. It always attempts to reap a proven-exited child, and preserves
  exact recovery captures whenever exit or cleanup cannot be proved.

### T-11706: Prove all clauses with real races, hardened fixtures, native smokes, and two-OS CI

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/tests/server_lifecycle_fixture.rs`,
  `crates/ferric-cli/src/bin/ferric_lifecycle_fixture.rs`,
  `crates/ferric-cli/src/server.rs`,
  `crates/ferric-cli/src/server_registration.rs`,
  `crates/ferric-cli/src/server_process.rs`, `crates/ferric-cli/Cargo.toml`,
  `.github/workflows/ci.yml`, `docs/server-configuration.md`,
  `docs/commands.md`, `docs/sprints/s117/sprint-tests/`
- **Depends on:** T-11701, T-11702, T-11703, T-11704, T-11705
- **Acceptance criterion:** INT-0008 AC-3, AC-4, AC-6, AC-7, and enabling evidence toward AC-9; T-11606 E05-C and complete clause evidence.
- **Success criterion (EARS):**
  - **E05-C — WHEN** the model-free cross-workspace CLI proof runs,
    **THEN** an isolated feature-gated Rust fixture copied as `llama-server`
    into the child process's temporary `PATH` **SHALL** accept the ordinary
    closed-engine argv and dummy regular model, serve loopback health, exercise
    up/status/down and stale-local/live-global recovery, and leave no owned
    process, listener, registration, stage,
    coordination artifact, or unrelated mutation.
- **Notes:** Begin every fixture lifetime guard before a blocking CLI call;
  add a bounded child watchdog; serialize lifecycle cases; retry only a
  diagnosed address-in-use bind; serve fixture connections independently; and
  use valid alternate OS tokens. Add a dedicated Ubuntu/Windows feature job
  with `--test-threads=1` and a bounded timeout. Compile-check the feature test
  surface for AArch64 without claiming native runtime execution. The ordinary
  workspace gates remain required.

## Definition of Done

- All nineteen finalized acceptance names exist and execute under their
  recorded commands; supplemental defect regressions trace to the same clauses.
- `cargo fmt --check`, clippy with warnings denied, the focused/default
  workspace suite, the feature-gated fixture suite, and native target smokes
  pass where applicable.
- Ubuntu and Windows lifecycle CI jobs pass on the immutable implementation
  head; the AArch64 feature surface compile-checks.
- Sprint Test artifacts record clause-level commands and outputs, and the Test
  critic returns `clean` or an explicitly resolved `proceed-with-caveats`.
- T-11606 moves to completed only after the Loop Phase verifies all evidence;
  otherwise Sprint 117 fails closed and later local-model tasks remain blocked.
