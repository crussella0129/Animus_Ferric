# Sprint 41 Test Report — container architecture (design + live-validated)

## Summary
A design/skeleton sprint with no Rust code, so no `cargo test` surface. Validation was structural
(Python checks on the Dockerfile/compose artifacts) plus — thanks to the mid-sprint Docker install —
**live** (`docker build`, `docker compose config`, `docker run`). A foreground test-critic
**independently re-ran every load-bearing check** against a live Docker engine (including a full
clean image rebuild and a live check of the llama-server download URL) and returned **clean** — all
claims held. All 5 build tasks' EARS clauses have a documented, passing check.

## The headline result: live validation caught a real bug a green build would have shipped
The Dockerfile compiled and the image built successfully (`docker build` EXIT 0) — but that was NOT
enough. Running the image (`docker run --rm ferric-core:s41 --version`) surfaced a runtime
`GLIBC_2.39 not found` error: the build stage (`rust:1.96-slim`, Debian-trixie, GLIBC 2.39) and
runtime stage (`debian:bookworm-slim`, GLIBC 2.36) had mismatched GLIBC. Fixed by pinning the build
stage to `rust:1.96-slim-bookworm`; re-run confirmed `ferric 0.1.0` runs. This is the concrete
argument for why the Docker install mattered — structural checks and even a green build could not
have caught it; only actually running the image did.

## Coverage by task
- **T-4101** (ADR-051): docs — verified by read (and grep, by the critic: co-location, the DinD
  correction, and all deferrals present).
- **T-4102** (`docker/Dockerfile`): structural checks pass (2 stages, `COPY --from` resolves,
  `--features backend-openai` present, no `EXPOSE`) + a live `docker build` producing a runnable
  206MB `ferric-core:s41` image.
- **T-4103** (`docker/docker-compose.yml`): structural checks pass (YAML valid, `ferric-core` the
  only active service with a resolving dockerfile path and no `ports:`, stubs commented) + live
  `docker compose config` EXIT 0.
- **T-4104** (docs corrections): `docs/ornstein.md`, ADR-051, and `agent-tasks.md` all name the
  corrected microVM-sandbox mechanism; zero stale `bollard` references remain (grep-verified).
- **T-4105** (live validation): the `docker build` + `docker run` smoke that found and drove the
  GLIBC fix; `llama-server` present + versioned in-container; runs non-root.

## Critic findings and resolutions
- **C-001** (reject): a naive substring search for the broken `rust:1.96-slim` false-positives on
  `rust:1.96-slim-bookworm` — a re-verification caution only; the fix itself is real and live-proven.
- **C-002** (tighten-claim, applied): `docker compose config` emits Compose's implicit `networks:
  default` bridge — an internal bridge, not a host-published port, so the loopback-only claim holds;
  a precision note was added to integration-tests.md so a reader doesn't misread it.

## Deliberately deferred (honest scope, per ADR-051)
- Running `ferric-core` end-to-end with a live model (`compose up` + a real `ferric query` through
  the containerized backend) — legitimately needs a mounted GGUF not present in this environment;
  the skeleton proves the plumbing assembles and runs, not that a full query completes.
- Multi-arch images (buildx `--platform`); a CI `docker` gate; Ornstein's own container process;
  chat mode (sprint 42).

## Confidence
Clean — proceed to Loop Phase.
