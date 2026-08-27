Finalized - DO NOT EDIT

# Sprint 41 Test Plan

This sprint produces no Rust code — no `cargo test` surface. Validation is of the design artifacts.
**Docker was installed and its `linux/x86_64` engine started mid-sprint** (see
`sprint-plans/critique.md`), so live validation IS now possible — this plan does REAL `docker
build`/`docker compose config`, not just Python structural checks. On this machine, invoke Docker
by full path (`C:\Program Files\Docker\Docker\resources\bin\docker.exe`) or from a fresh shell.

## Unit Tests (structural — the cheap fast checks, run first)
### T-4102 (`docker/Dockerfile`) structural checks
- Every `FROM ... AS <name>` label referenced by a later `COPY --from=<name>` exists earlier in
  the file (name-resolution check).
- The build stage contains `--features backend-openai` (plan-critic C-001 — without it the built
  `ferric` can't drive the co-located backend).
- No `EXPOSE`/port-publish directive appears (loopback-only intent preserved structurally).
- Run via a short Python script (no `hadolint` in this environment).

### T-4103 (`docker/docker-compose.yml`) structural checks
- `python -c "import yaml; yaml.safe_load(open('docker/docker-compose.yml'))"` — YAML validity.
- `ferric-core` is the only service with a resolvable `build:` block whose `dockerfile:` value
  string-matches the real `docker/Dockerfile` path created in T-4102 (plan-critic C-004: a concrete
  scriptable string-match, not a vague "consistency" judgement); `ferric-core` has no functional
  `ports:` directive (plan-critic C-006).
- `ornstein-search`/`chat` carry an explicit stub marker (commented out or `# STUB —` prefix),
  never a real `build:`/`image:`.

## Integration Tests
### Cross-artifact consistency (plan-critic C-004 — tightened to scriptable string assertions)
- Both `docs/ornstein.md`'s corrected container/proxy line and ADR-051 (in `decisions.md`) contain
  the literal strings `Docker Sandboxes` and `gVisor` (the corrected mechanism named consistently
  in both), and neither reintroduces a bare `bollard` as an isolation mechanism.
- `agent-tasks/agent-tasks.md` no longer contains the stale `(bollard/gVisor)` phrase (plan-critic
  C-005).

### T-4105 live validation (the strongest checks — Docker is installed)
- `docker build -f docker/Dockerfile .` completes successfully and produces a runnable image (this
  actually compiles the `ferric` binary with `--features backend-openai` and installs
  `llama-server` — a real end-to-end build of the `ferric-core` artifact, not an approximation).
- `docker compose -f docker/docker-compose.yml config` validates and emits the resolved config
  without error (real compose-schema validation, superseding the daemon-less Python YAML parse).

## End-to-End Tests
- **Status:** possible (Docker now installed) — the T-4105 `docker build` + `docker compose config`
  checks above ARE the E2E validation for a design-artifact sprint. Filed under Integration Tests
  (T-4105) rather than duplicated here, matching sprints 38–40's precedent of not duplicating a
  check across sections just to satisfy a heading.
- **Deliberately still deferred:** actually RUNNING the `ferric-core` container end-to-end with a
  live model behind it (`docker compose up` + a real `ferric query` through the containerized
  backend) — that needs a GGUF model mounted and is the natural first step of whatever sprint
  actually operationalizes the container, not this design/skeleton sprint. A CI `docker compose
  config` gate (plan-critic C-003's suggestion — GitHub Actions `ubuntu-latest` has Docker) is
  recorded in the ADR/test-report as an available future hardening, not added this sprint (a full
  workspace-compile-in-container on every PR is disproportionate for skeleton artifacts).

## Build/Lint (all tasks)
No `cargo` surface (no Rust code changed). The structural + live Docker checks above are the
validation surface.
