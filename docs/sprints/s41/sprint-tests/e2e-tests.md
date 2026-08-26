# Sprint 41 E2E Tests

- **Status:** the strongest end-to-end check possible this sprint IS the live `docker build` +
  `docker run` smoke of `ferric-core:s41` (documented under `integration-tests.md`'s T-4105
  section rather than duplicated here, matching sprints 38–40's precedent of not repeating a check
  across sections just for a heading). It proves the built image is a real, runnable artifact: the
  `ferric` binary executes (`ferric 0.1.0`), its co-located `llama-server` backend is present and
  versioned, and the container runs non-root — the concrete validation of ADR-051's co-location
  design.
- **Deliberately still deferred** (named, per ADR-051): actually RUNNING `ferric-core` end-to-end
  with a live model behind it — `docker compose up` + a real `ferric query` driving the
  containerized `llama-server` against a mounted GGUF — is the natural first step of whatever
  sprint OPERATIONALIZES the container, not this design/skeleton sprint. It needs a model file
  mounted into the container and a real inference run; the skeleton proves the plumbing assembles
  and runs, not that a full agentic query completes inside it.
- **A CI `docker compose config` / `docker build` gate** (plan-critic C-003 — GitHub Actions
  `ubuntu-latest` ships Docker) is recorded in ADR-051 as an available future hardening, not added
  this sprint: a full workspace-compile-in-container on every PR is disproportionate for skeleton
  artifacts, and the artifacts were already validated live locally.
