# Agent Tasks (Persistent Backlog)

> **Direction change in s30:** began the **Animus** suite by **hardening Animus Loop**.
> Sprint 30 shipped **Ornstein increment 1** — the quarantined summarizer (`ferric-research`):
> untrusted content → a no-tools/no-memory model under a data-only schema → a typed,
> provenance-tagged `ResearchDigest`. The quarantine is *structural* (reuses the constrained
> valve), so injections can only surface as quoted data. 4 tests incl. the injection-
> containment proof. ADR-040; `docs/ornstein.md`. PR cadence clean.

## Animus vision (recovered from the user, 2026-06-27)
- **Animus Launch** — interactive Rust+scripts project bootstrapper (successor to GECK's launcher: `crates/geck-cli/wizard.rs` + `geck_generator`). Interview → git repo with main+dev → "begin work?". **Decision: lives as a crate in Animus_Ferric.**
- **Animus Loop** — the sprint-loops protocol (Research→Plan→Build→Test→Loop). **First priority = harden it.**
- **Animus Manage** — multi-agent project-management layer (least-specified).

## Ornstein = a quarantined MULTI-SOURCE research subsystem (user's expanded vision)
One funnel (the built quarantine), many pluggable `Retriever`s (capability-probed). **Build order (user-chosen):**
- **inc 2 — `Retriever` trait + Local FS retriever** ✅ **DONE (s31, ADR-041).** Keystone `Retriever` (plane/available/retrieve, async) + `LocalFsRetriever` + `research()` pipeline; source→funnel→digest proven.
- **inc 3 — Tailnet/NAS FS retriever** ✅ **DONE (s32, ADR-042).** `TailnetFsRetriever` searches a remote tailnet device's FS over SSH (`SshTransport::{Tailscale, Plain{port}}`); query single-quote-escaped vs remote command injection; `parse_status_devices` for `available()`. Deterministic core tested; **live SSH E2E deferred** (no target's sshd was up — Pixel has none on :22/:8022, switchblade offline).
- **inc 3b — live SSH E2E for the tailnet plane** ← run once a target's sshd is up (Termux `Plain{8022}` on pixel-10-pro-xl, or `Tailscale` on switchblade when back online): `research(&TailnetFsRetriever{…}, provider, query)` → quarantined `host:path` digests.
- **research orchestrator** ✅ **DONE (s33, ADR-043).** `research_all(planes, provider, query) -> MultiResearch` runs a query across all available planes, chunk-level dedup by source (one model call per source), per-plane `PlaneResult` report; unavailable planes are recorded no-ops.
- **inc 4 — Web retriever + hardened container + allowlist proxy** (bollard/gVisor) ← **NEXT, but BLOCKED on a containerizer** (no docker/podman on Windows or WSL as of s33). Install Docker Desktop (elevated `winget install Docker.DockerDesktop`) OR docker.io in WSL2 (`sudo apt install docker.io`) to unblock. The online plane; the trifecta's exfil leg lives here, so its security layer comes last.
- **CaMeL-lite sink-policy primitive** ✅ **DONE (s34, ADR-044, co-designed with the user).** `crates/ferric-research/src/sink.rs`: `TaintSet` (substring taint over digest summary+quotes) + `SinkPolicy::decide(permission, tainted)` keyed off `PermissionLevel`, all 3 modes (`Deny`/`RequireApproval`/`Warn`, caller picks). 8 tests incl. the end-to-end gate shape. **Pure primitive — NOT wired into dispatch yet.**
- **inc 5 (remaining) — wire the sink policy into `registry.execute` + Loop research-phase wiring** ← run when the research→loop integration lands: populate a `TaintSet` from digests entering context; call `SinkPolicy::decide` beside the existing `check(permission, path)` at the dispatch chokepoint; route fetched content through the quarantine before the planner acts (a sprint-loops change).
- **PR open+merge as the STANDARD final loop phase** — promote sprint-loops `06-loop-phase.md` step #6 from optional to standard (small, clear; matches one-PR-per-sprint).
- **A testing system** — make the Loop's Test phase a real system (define first: containerized test runs? golden artifacts? coverage gates?).
- **Animus Launch (crate in Animus_Ferric)** — the interactive bootstrapper, once loop-hardening has momentum.
- A live small-model run measuring Ornstein summarization *quality* (safety is already structural).

## Earlier backlog (still open)
- Multi-file `apply_patch` (ADR-039 follow-on); GPU/edge run (Jetson/Pi → maybe L6); harder bench L7+; MCP-stdio (ADR-012); audio on real non-TTS audio + video modality.
