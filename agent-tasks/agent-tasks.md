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
- **inc 3 — Tailnet/NAS FS retriever** ← **NEXT.** A `Retriever` that reaches a NAS + LAN devices over **Tailscale** (LocalAPI `/status` enumerate, `whois` identity, SSH/`serve` reach), searches their filesystems (substrate pre-scoped in s1 `docker-nix-tailscale.md`). Same trait shape as `LocalFsRetriever`, reached over the network.
- **inc 4 — Web retriever + hardened container + allowlist proxy** (bollard/gVisor) — the online plane; the trifecta's exfil leg lives here, so its security layer comes last.
- **inc 5 — CaMeL taint/sink-policy + research orchestrator + Loop research-phase wiring** (route fetched content through the quarantine before the planner sees it; a sprint-loops change).
- **PR open+merge as the STANDARD final loop phase** — promote sprint-loops `06-loop-phase.md` step #6 from optional to standard (small, clear; matches one-PR-per-sprint).
- **A testing system** — make the Loop's Test phase a real system (define first: containerized test runs? golden artifacts? coverage gates?).
- **Animus Launch (crate in Animus_Ferric)** — the interactive bootstrapper, once loop-hardening has momentum.
- A live small-model run measuring Ornstein summarization *quality* (safety is already structural).

## Earlier backlog (still open)
- Multi-file `apply_patch` (ADR-039 follow-on); GPU/edge run (Jetson/Pi → maybe L6); harder bench L7+; MCP-stdio (ADR-012); audio on real non-TTS audio + video modality.
