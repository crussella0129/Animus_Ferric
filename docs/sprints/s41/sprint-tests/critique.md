# Test Critique — Sprint 41

Reviewed by a foreground test-critic agent that **independently re-ran every load-bearing check**
against a live Docker engine (server 29.6.1) — including a full clean rebuild of the image from the
current Dockerfile (EXIT 0, ran `ferric 0.1.0`), a live `curl -sIL` on the llama-server download
URL (HTTP 200), and grep-verification of the doc-consistency claims. **Confidence: clean.** Zero
substantive concerns; two minor optional observations.

## C-001: naive substring match for the GLIBC fix could false-positive on re-verification
- **Finding:** unit-tests.md correctly claims the fix pins `rust:1.96-slim-bookworm` (verified
  true — the live image runs `ferric 0.1.0` with no GLIBC error). The only caution: a naive
  substring search for the *broken* `rust:1.96-slim` would false-positive, since
  `rust:1.96-slim-bookworm` contains it as a prefix. The test files never claim to use such a
  regex, so this is only advice for anyone re-verifying.
- **Response:** **reject as a defect** (the fix is genuine and live-proven). Noted here for
  reproducibility: re-verify by exact-`FROM`-line match, not substring.

## C-002: `docker compose config` emits `networks: default` — precision note so it's not misread
- **Finding:** integration-tests.md says the resolved config shows "no `ports:` — matches the
  design" (confirmed true — EXIT 0, no `ports:`). Compose additionally auto-synthesizes a
  `networks: default: name: docker_default` (its implicit default bridge, present for any service).
  This is an internal bridge, **not a host-published port**, so the loopback-only claim is intact —
  but "no `ports:`" could read as "no networking at all."
- **Response:** **tighten-claim** (applied). Added a one-line note to integration-tests.md that the
  only network is Compose's implicit default bridge with no host port published — the central
  security claim (no host-published port) is verified intact.

## Independently verified by the critic (all PASS)
- Structural checks re-run (Dockerfile 2 stages + `COPY --from` resolves + `--features
  backend-openai` + no `EXPOSE`; compose YAML valid + `ferric-core` only active service + dockerfile
  path resolves + no `ports:` + stubs commented) — match unit-tests.md exactly.
- `ferric-core:s41` exists (206MB); `docker run --rm ferric-core:s41 --version` → `ferric 0.1.0`;
  GLIBC fix real (current Dockerfile uses `rust:1.96-slim-bookworm`). A full clean rebuild also
  succeeded and ran.
- `llama-server` present at `/opt/llama-b9821/llama-server`, on PATH, `version: 9821`; container
  runs non-root (`uid=10001(ferric)`).
- The b9821 ubuntu-x64 `.tar.gz` download URL returns HTTP 200 (a clean build resolves it).
- Doc consistency grep-verified: `docs/ornstein.md` + ADR-051 both name `Docker Sandboxes` +
  `gVisor`; `agent-tasks.md` has zero `bollard` matches and records the blocker cleared.
- EARS coverage complete for T-4101–T-4105; deferral honesty confirmed (running with a live model
  legitimately needs a mounted GGUF not in this environment).

## Confidence
clean → the two optional notes addressed (C-001 reject-with-note, C-002 precision tweak applied);
no re-verification needed.
