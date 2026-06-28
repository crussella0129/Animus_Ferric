# Sprint 30 Meta

- **Sprint number:** 30
- **Start timestamp:** 2026-06-27T21:59:53Z
- **End timestamp:** 2026-06-27T22:55:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** **PIVOT — began the Animus suite by hardening Animus Loop; Ornstein increment 1: the quarantined summarizer.** Recovered Ornstein from the s1 research (`docker-nix-tailscale.md`) + ADR-014 roadmap (deferred "s3+", never built). New crate `crates/ferric-research`: `ResearchDigest`/`Claim` (data-only), `digest_schema()`, `summarize_quarantined()` — untrusted content → a model with **no tools, no memory** under a data-only JSON-Schema constraint → a typed, provenance-tagged digest. The quarantine is **structural** (reuses the constrained valve; ADR-010 makes empty-tools the only valid constrained shape), so an injection can only surface as quoted data; provenance (`source`/`untrusted`) is harness-stamped so the model can't launder its taint. 4 tests incl. the injection-containment proof. Container/proxy + CaMeL sink-policy + network fetch + Loop wiring deferred (ADR-040). `docs/ornstein.md`; README Status 30. One PR per sprint; `dev` clean (PR #15 merged). Sprint originally scoped to multi-file apply_patch, restarted on user redirect.
