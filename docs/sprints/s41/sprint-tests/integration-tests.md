# Sprint 41 Integration Tests (live Docker validation)

Docker Desktop was installed mid-sprint (`linux/x86_64` engine, WSL2 backend), so the artifacts
were validated LIVE, not just structurally. This is the authoritative validation surface for a
design/skeleton sprint.

## Cross-artifact consistency (plan-critic C-004/C-005 — scriptable string checks)
- `docs/ornstein.md`'s corrected container/proxy line and ADR-051 (`decisions.md`) both name the
  microVM-sandbox mechanism (`Docker Sandboxes` + `gVisor`) — consistent, no divergence; neither
  reintroduces `bollard` as an isolation mechanism.
- `agent-tasks/agent-tasks.md` no longer carries the stale `(bollard/gVisor)` phrase nor the stale
  "BLOCKED on a containerizer / install Docker" text (Docker is now installed).
- `docker-compose.yml`'s `ferric-core.build.dockerfile` resolves to the real `docker/Dockerfile`.

## T-4105 — live `docker compose config` + `docker build` + smoke run
- **`docker compose -f docker/docker-compose.yml config`** → EXIT 0; resolved config shows only
  `ferric-core` active, `dockerfile: docker/Dockerfile`, no `ports:` — matches the design. (The
  resolved config does include `networks: default` — Compose's implicit default bridge, present
  for any service; it is an internal bridge, NOT a host-published port, so the loopback-only claim
  holds. Test-critic C-002 precision note.)
- **`docker build -f docker/Dockerfile -t ferric-core:s41 .`** → EXIT 0. Compiled the whole
  workspace in release with `--features backend-openai` (confirmed: `reqwest`/`rustls-webpki`/
  `hyper-rustls` — the backend-openai deps — compiled; `Finished release profile ... in 57.98s`),
  installed the prebuilt `llama-server` b9821 Linux-x64 release, produced `ferric-core:s41`
  (206 MB).
- **Smoke run — the check that earned live validation its keep.** A green `docker build` was NOT
  sufficient: `docker run --rm ferric-core:s41 --version` surfaced a real runtime bug the build
  alone could never catch — `ferric: /lib/x86_64-linux-gnu/libc.so.6: version 'GLIBC_2.39' not
  found`. Root cause: build stage `rust:1.96-slim` (Debian-trixie, GLIBC 2.39) vs runtime
  `debian:bookworm-slim` (GLIBC 2.36), a dynamic-link mismatch. **Fixed** by pinning the build
  stage to `rust:1.96-slim-bookworm`. After the fix, re-run confirmed:
  - `docker run --rm ferric-core:s41 --version` → `ferric 0.1.0` ✅
  - `docker run --rm ferric-core:s41 --help` → the real CLI usage ✅
  - `llama-server` present at `/opt/llama-b9821/llama-server`, on PATH, reports `version: 9821` ✅
  - `id` inside the container → `uid=10001(ferric)` (non-root) ✅

## Result
All live checks green after the GLIBC fix. The `ferric-core:s41` image is a real, runnable
artifact: the harness binary executes, its co-located inference backend is present and versioned,
and it runs non-root — the concrete proof the co-location design (ADR-051) actually works, not just
on paper.
