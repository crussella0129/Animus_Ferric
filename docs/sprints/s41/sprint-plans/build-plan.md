Finalized - DO NOT EDIT

# Sprint 41 Build Plan

## Schema Tree
- Sprint Goal: container architecture (design only) — correct the DinD framing, decide the
  sibling-container vs. microVM-sandbox split, produce a concrete starting artifact set
  - Decision record
    - T-4101: ADR-051 — container architecture
  - Skeleton artifacts
    - T-4102: `docker/Dockerfile` — the `ferric-core` image
    - T-4103: `docker/docker-compose.yml` — the sibling-container topology skeleton
  - Docs wrap-up
    - T-4104: `docs/ornstein.md` correction + ADR-051 docs wrap-up
  - Live validation (unblocked mid-sprint — Docker now installed)
    - T-4105: live `docker build` + `docker compose config` of the skeleton artifacts

## Execution Sequence

### T-4101: ADR-051 — container architecture (sibling containers + microVM airlock, not DinD)
- **Touches:** `decisions.md`
- **Depends on:** (none)
- Records: the DinD correction and why (cited research — literal Docker-in-Docker is now a
  security anti-pattern for isolating untrusted content specifically; 2026 industry practice,
  including Docker's own "Docker Sandboxes" product, converges on microVM-class sandboxes for
  that use case); the sibling-containers-for-deployment vs. microVM-sandbox-for-airlock split
  (two different tools for two different problems the user's original framing conflated); the
  loopback/backend-colocation finding (`ferric server`'s ADR-005 loopback-only pin means `ferric` +
  its inference backend should stay co-located in ONE container, `ferric-core`, rather than being
  split across containers without extra network-namespace plumbing) and its recommendation;
  explicit deferrals (whether `ferric-research` gets its own binary/service entrypoint, MCP's own
  containerization question, chat mode's own build/ADR — sprint 42, live testing — blocked on
  Docker installation) each with one-sentence rationale.
- **Success criterion (EARS):**
  - **WHEN** ADR-051 is read, **THEN** it **SHALL** state the DinD-vs-microVM-sandbox correction
    with its rationale, the sibling-containers-for-deployment decision, the loopback-driven
    `ferric-core` co-location recommendation, and explicitly list all deferred decisions with
    one-sentence rationale each.

### T-4102: `docker/Dockerfile` — the `ferric-core` image (multi-stage: `ferric` + backend)
- **Touches:** new file `docker/Dockerfile`
- **Depends on:** T-4101
- Multi-stage build: a build stage compiling the `ferric` binary from the workspace with
  **`cargo build --release --features backend-openai -p ferric-cli`** (plan-critic C-001:
  `ferric-cli`'s `default = []`, so a plain release build produces a binary that CANNOT drive the
  co-located `llama-server` over the HTTP valve — the OpenAI backend feature is mandatory for the
  co-location recommendation to mean anything), matching the pinned toolchain in
  `rust-toolchain.toml`. A runtime stage on a slim base image installing a prebuilt `llama-server`
  release binary (mirroring `docs/llama-cpp.md`'s existing documented installation method — the
  CPU-x64 release asset — no new backend-acquisition mechanism invented) alongside the compiled
  `ferric` binary, so `ferric server up` still targets loopback correctly INSIDE this one
  container. **x86_64/Linux only for this skeleton** (plan-critic C-002: the project has a
  CI-gated aarch64 portability ambition — ADR-004 — but `docs/llama-cpp.md`'s install is
  asset-selection-by-hardware, so the skeleton targets one arch deliberately; multi-arch via
  buildx `--platform` is the named future path, deferred, not silently x86-only). No
  `EXPOSE`/port-publish directive (preserves loopback-only intent structurally). Comments explain
  each stage's purpose, the x86_64 scope-limit, and cross-reference ADR-051.
- **Success criterion (EARS):**
  - **WHEN** `docker/Dockerfile` is read, **THEN** it **SHALL** define a build stage that invokes
    `cargo build --release --features backend-openai` and a runtime stage containing both the
    resulting `ferric` binary and a `llama-server` installation, with no public port exposed by
    default.
  - **WHEN** the file is structurally checked (multi-stage `FROM ... AS <name>` labels,
    `COPY --from=<name>` references resolving to a real earlier stage, presence of
    `--features backend-openai`, absence of `EXPOSE`), **THEN** it **SHALL** pass all four checks.
  - **WHEN** `docker build -f docker/Dockerfile .` is run (Docker is now installed — see
    critique.md), **THEN** it **SHALL** complete successfully, producing a runnable `ferric-core`
    image (the strongest available validation; supersedes the originally-planned structural-only
    check).

### T-4103: `docker/docker-compose.yml` — the sibling-container topology skeleton
- **Touches:** new file `docker/docker-compose.yml`
- **Depends on:** T-4102
- One fully-real service (`ferric-core`, `build: context: .., dockerfile: docker/Dockerfile`, a
  workspace-mounted volume). Future inter-service reachability is documented as a **COMMENT only —
  no functional `ports:`/`networks:` publish directive** (plan-critic C-006: a live `ports:`
  mapping would contradict T-4102's deliberate no-`EXPOSE` loopback-only stance in the same
  sprint). `ornstein-search` and `chat` appear as clearly-marked STUB service blocks (commented
  out or carrying an explicit `# STUB —` prefix; no `build:`/`image:` resolving to anything real),
  each with a one-line comment naming the future sprint/ADR that would flesh it out. A top-level
  comment block explains the topology decision and cross-references ADR-051.
- **Success criterion (EARS):**
  - **WHEN** `docker/docker-compose.yml` is parsed by a standard YAML parser (and by `docker
    compose config`, now that Docker is installed), **THEN** it **SHALL** parse/validate without
    error.
  - **WHEN** the file is read, **THEN** the `ferric-core` service **SHALL** be the only fully
    defined (buildable) service with no functional `ports:` directive; `ornstein-search`/`chat`
    **SHALL** be clearly marked as stubs, never silently presented as functional.

### T-4104: `docs/ornstein.md` correction + ADR-051 docs wrap-up
- **Touches:** `docs/ornstein.md`, `README.md`, `agent-tasks/agent-tasks.md`,
  `agent-tasks/completed-tasks.md`
- **Depends on:** T-4101–T-4103
- `docs/ornstein.md`'s "Web `Retriever` + hardened container + allowlist egress proxy" line
  (currently naming "bollard/gVisor" without distinguishing orchestration-client from isolation-
  mechanism) is corrected to name the microVM-sandbox recommendation (Docker Sandboxes primary,
  gVisor as a Linux-native alternative) with a one-line rationale, cross-referencing ADR-051.
  **`agent-tasks/agent-tasks.md`'s own "(bollard/gVisor)" backlog phrase is corrected in the same
  pass** (plan-critic C-005: it independently carries the identical stale phrasing — folded into
  the sprint-41 backlog-section rewrite so the two files don't diverge). **The multi-sprint "USER
  must install Docker" blocker text in `agent-tasks.md`/`docs/ornstein.md` is updated to reflect
  that Docker is now installed** (it was cleared mid-sprint — see critique.md). README Status
  bumped to sprint 41 + a new timeline entry; the sprint 41 backlog section rewritten from
  in-progress to a completed summary (matching sprints 38–40's precedent), explicitly noting chat
  mode's deferral to sprint 42.
- **Success criterion (EARS):**
  - **WHEN** `docs/ornstein.md`'s container/proxy line AND `agent-tasks/agent-tasks.md`'s backlog
    line are read, **THEN** both **SHALL** name the corrected microVM-sandbox recommendation
    instead of the prior undifferentiated "bollard/gVisor" mention, with no divergence between
    them.

### T-4105: live `docker build` + `docker compose config` validation
- **Touches:** (no repo files — a validation task; findings recorded in the Test Phase report)
- **Depends on:** T-4102, T-4103
- **Unblocked mid-sprint:** Docker Desktop was installed and its `linux/x86_64` engine started
  during the plan critique (see `critique.md`), so the skeleton artifacts can be validated for
  REAL, not just structurally. `docker build -f docker/Dockerfile .` actually compiles the
  `ferric` binary (with `--features backend-openai`) and assembles the runtime image; `docker
  compose -f docker/docker-compose.yml config` validates the full compose topology. On this
  machine, invoke Docker by full path (`C:\Program Files\Docker\Docker\resources\bin\docker.exe`)
  or from a fresh shell session (the CLI isn't on an already-running Git-Bash session's PATH).
- **Success criterion (EARS):**
  - **WHEN** `docker build -f docker/Dockerfile .` is run against T-4102's Dockerfile, **THEN** it
    **SHALL** complete successfully and produce a runnable image.
  - **WHEN** `docker compose -f docker/docker-compose.yml config` is run against T-4103's compose
    file, **THEN** it **SHALL** validate and emit the resolved config without error.
  - **WHEN** either live check fails, **THEN** the underlying artifact **SHALL** be fixed and
    re-validated before the sprint closes (a real build gate, not a structural approximation).
