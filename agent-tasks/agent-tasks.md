# Agent Tasks (Persistent Backlog)


## Capability roadmap (ADR-014 — pinned, not aspiration)

### s2 — prompt assembly, action grammar, calibration
- [ ] (s2) oovra as a versioned crate dependency: per-tier/per-protocol system-prompt composition under RunPolicy; prompt genealogy in trace events
- [ ] (s2) Unified action grammar: one JSON-Schema constraint covering tool choice + task_complete, harness-owned end-to-end (revisits ADR-010 split; needs strict-field mistralrs bump or llguidance grammar composition)
- [ ] (s2) HTTP escape-valve backend (OpenAI-compatible llama-server/Ollama client) with bounded reads (Prion #3); JsonSchema-only constraints initially
- [ ] (s2) Circuit-breaker compaction in a context manager (Fev pattern)
- [ ] (s2) Stale-config detection/migration in the config layer (Animus H20)
- [ ] (s2) Port the L0–L6 benchmark harness from Animus; measure the fleet; calibrate the tier table (feeds measured_level)
- [ ] (s2) Per-turn output-token budget in RunPolicy: the policy caps turns but not generation length (SamplingParams default max_tokens=2048 made a single debug-profile turn run 37+ min in s1); tier table should scale max_tokens like it scales turns

### s3 — integration surfaces + sandbox substrate
- [ ] (s3) GECK absorption: `ferric init-project --profile X` (Rust-native templates; GECK profiles as oovra-compatible elements)
- [ ] (s3) `ferric mcp` stdio server (typed tools: query/status/trace) + minimal SKILL.md companion (ADR-012)
- [ ] (s3) Docker capability layer via bollard: hardened throwaway containers (network none, cap-drop ALL, read-only rootfs, runsc opportunistic)
- [ ] (s3) Nix capability probe + dockerTools image composition (WSL2 path on Windows)
- [ ] (s3) Ornstein quarantine start: container-isolated retrieval + quarantined local-model summarizer + provenance-tagged outputs (dual-LLM + CaMeL-lite)

### s4–s7 — the Development Engine
- [ ] (s4–s7) ferric-engine: Rust-native port of the sprint-loops five-phase protocol (filesystem state machine, phase routing, critics, confidence throttle gating RunPolicy); becomes `ferric dev` (ADR-011)

### s3+ — tailnet surface
- [ ] (s3+) Tailscale LocalAPI integration (status/whois via named pipe/socket), `tailscale serve` exposure, identity-based authz inside Ferric; never funnel

## User-flagged research leads (2026-06-11, must not evaporate)
- [ ] (s2+ research) Re-examine "fully Rust implementable" tree-sitter: user pointer https://github.com/tree-sitter/tree-sitter-rust . Note: that repo is the Rust *grammar* for tree-sitter (generated C parser for parsing Rust source), not a Rust runtime — but the underlying question stands: revisit whether Ferric's parsing layer can be 100% Rust (per-language pure-Rust parsers like syn for Rust; tree-sitter-c2rust transpilation; wasm-sandboxed grammars; or accepting the named C boundary per ADR-013). Re-research with fresh 2026 ecosystem state before s3.
- [ ] (s2+ design) Ownership-graph attestation: the 100%-Rust goal exists so the ownership/lifetime/borrowing system is fully auditable and can be COMPARED TO AN IMMUTABLE ARTIFACT in the remote repo — an internal chain of trust over memory. Design a committed, CI-verified artifact (cargo-tree/SBOM + vet/audit-style lockfile listing every crate, its language composition, and every named non-Rust boundary) that any build can be diffed against. Candidate tools: cargo-auditable, cargo-vet, cargo-deny, cargo-sbom. (Referenced by ADR-013.)

## Lineage-fix ledger status (updated s1)
- [x] Hash-ALL-calls repetition guard (Prion #5) — DONE s1 T-106
- [x] Structured terminator task_complete (Animus) — DONE s1 T-105 (grammar wiring of the terminator is part of the s2 unified action grammar)
- [x] Exponential backoff on retryable errors (Prion #6) — DONE s1 T-107
- [x] First real Provider backend + L0 smoke (ADR-009 gate) — DONE s1 T-109/T-112
- [ ] Bounded reads on HTTP responses (Prion #3) — s2, with the HTTP backend
- [ ] Stale-config detection/migration (Animus H20) — s2, with the config layer
- [ ] Circuit-breaker compaction (Fev) — s2, with the context manager
