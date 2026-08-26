# Plan Critique — Sprint 41

Reviewed by a foreground plan-critic agent against `research-report.md`, `decisions.md`, and the
real current source (`server.rs`, `docs/llama-cpp.md`, `docs/ornstein.md`, `ferric-cli/Cargo.toml`,
CI workflow). The ADR-005 loopback claim and the ADR-040–044 incremental-sequencing precedent both
verified accurate. ADR-051 confirmed as the next free number.

> **Mid-critique platform change:** the user installed Docker Desktop and started the engine
> (`linux/x86_64`, WSL2 backend, confirmed working — CLI 29.6.1 / buildx 0.35 / compose 5.2). This
> clears the multi-sprint "no containerizer" blocker the whole plan was written around, and
> directly resolves C-003 (live validation IS now possible) — folded into the responses below.

## C-001: the Dockerfile build stage would produce a `ferric` binary with NO working backend
- **Finding:** `crates/ferric-cli/Cargo.toml:14` sets `default = []`; `backend-openai` is
  feature-gated (verified). A plain `cargo build --release -p ferric-cli` (what T-4102's wording
  implied) produces a `ferric` that can't talk to the co-located `llama-server` over the HTTP valve
  — defeating the entire "co-locate in one container" recommendation. README states this warning
  explicitly.
- **Response:** **fix-in-plan.** T-4102's build stage now explicitly requires `cargo build
  --release --features backend-openai -p ferric-cli`; T-4102's success criterion and the test-plan
  both now check for `--features backend-openai` in the Dockerfile. (This is also now LIVE-testable
  — a real `docker build` will fail fast if the binary can't reach the backend.)

## C-002: no target architecture stated, despite the CI-gated aarch64 commitment
- **Finding:** `.github/workflows/ci.yml` runs an `aarch64-check` portability gate (ADR-004);
  `docs/llama-cpp.md` lists per-arch llama-server release assets (x64 vs arm64 vs CUDA/Vulkan) —
  "installation" is asset-selection-by-hardware, not one universal `RUN`. A Dockerfile that
  hardcodes one asset silently commits to x86_64-only.
- **Response:** **fix-in-plan.** T-4102 now states x86_64-only as a deliberate skeleton scope-limit
  (confirmed correct: the local engine is `linux/x86_64`), with an explicit one-line aarch64
  deferral note rather than silence. Multi-arch (buildx `--platform`) is named as the future path.

## C-003: test-plan's "no live validation possible" claim was broader than reality
- **Finding:** the critic noted GitHub Actions `ubuntu-latest` ships Docker, so CI could validate
  the artifacts even without a local install — the "not possible" framing conflated "not on this
  machine" with "not anywhere."
- **Response:** **fix-in-plan, now moot locally.** Docker is installed and running on this machine
  as of mid-critique (see banner above), so the Test Phase now does REAL live validation locally:
  `docker build -f docker/Dockerfile .` (the strongest possible check — actually compiles the
  binary and assembles the image) and `docker compose -f docker/docker-compose.yml config`. The CI
  option the critic raised is recorded in the test-report/ADR as an available future hardening
  (a `docker compose config` PR gate), not added as a task this sprint — proportionate for skeleton
  artifacts, and avoids a heavy full-workspace-compile-in-container on every PR.

## C-004: the cross-artifact consistency checks were too vague to script
- **Finding:** "state the SAME mechanism ... no contradiction" has no defined pass/fail procedure,
  unlike the genuinely scriptable Dockerfile/YAML checks.
- **Response:** **fix-in-plan.** Tightened to concrete string assertions: both `docs/ornstein.md`'s
  corrected line and ADR-051 SHALL contain the literal `Docker Sandboxes` and `gVisor`, and the
  `docker-compose.yml` `ferric-core` service's `dockerfile:` value SHALL string-match the real
  `docker/Dockerfile` path — all scriptable.

## C-005: `agent-tasks/agent-tasks.md` independently carries the same stale "bollard/gVisor" phrase
- **Finding:** `agent-tasks/agent-tasks.md` also has "(bollard/gVisor)"; T-4104's Touches list names
  the file but its body only described correcting `docs/ornstein.md`.
- **Response:** **fix-in-plan.** T-4104 now explicitly folds the `agent-tasks.md` backlog line's
  container-mechanism phrasing into the same correction pass (that section gets rewritten to the
  sprint-41 completed summary anyway, so it's corrected as part of that rewrite) — no divergence
  left between the two files.

## C-006: `docker-compose.yml`'s `ferric-core` "documented port/socket" risked a live publish path
- **Finding:** a concrete `ports:` mapping would contradict T-4102's deliberate no-`EXPOSE`
  loopback-only stance in the same sprint.
- **Response:** **fix-in-plan.** T-4103 now specifies the `ferric-core` block's future-reachability
  note is a COMMENT only — no functional `ports:`/`networks:` publish directive — consistent with
  T-4102's loopback-only structural stance and the STUB discipline applied to the other services.

## Confidence
proceed-with-caveats → all 6 concerns addressed in the revised build-plan.md/test-plan.md (5
fix-in-plan, 1 fix-in-plan-now-moot-locally); the Docker install additionally upgrades the whole
sprint from design-only to design + live-validated.
