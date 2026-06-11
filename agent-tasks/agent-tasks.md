# Agent Tasks (Persistent Backlog)

- [ ] T-111 (sprint 1): ferric query subcommand (--mock + real path) — touches: crates/ferric-cli/src/{main.rs,query.rs}
- [ ] T-112 (sprint 1): L0 smoke E2E (#[ignore], feature-gated, env-driven) — touches: crates/ferric-cli/tests/l0_smoke.rs
- [ ] T-113 (sprint 1): ADR-010..014 + backlog roadmap rewrite — touches: decisions.md, agent-tasks/

## User-flagged research leads (2026-06-11, must not evaporate)
- [ ] (s2+ research) Re-examine "fully Rust implementable" tree-sitter: user pointer https://github.com/tree-sitter/tree-sitter-rust . Note: that repo is the Rust *grammar* for tree-sitter (generated C parser for parsing Rust source), not a Rust runtime — but the underlying question stands: revisit whether Ferric's parsing layer can be 100% Rust (per-language pure-Rust parsers like syn for Rust; tree-sitter-c2rust transpilation; wasm-sandboxed grammars; or accepting the named C boundary per ADR-013). Re-research with fresh 2026 ecosystem state before s3.
- [ ] (s2+ design) Ownership-graph attestation: the 100%-Rust goal exists so the ownership/lifetime/borrowing system is fully auditable and can be COMPARED TO AN IMMUTABLE ARTIFACT in the remote repo — an internal chain of trust over memory. Design a committed, CI-verified artifact (cargo-tree/SBOM + vet/audit-style lockfile listing every crate, its language composition, and every named non-Rust boundary) that any build can be diffed against. Candidate tools: cargo-auditable, cargo-vet, cargo-deny, cargo-sbom.

## Deferred lineage fixes (from sprint 0 Lineage-Fix Ledger — must not evaporate)
- [ ] (s1) Hash-ALL-calls repetition guard in the agent loop (Prion #5)
- [ ] (s1) Structured terminator (task_complete) wired into constraint grammar (Animus)
- [ ] (s1) Exponential backoff on retryable provider errors (Prion #6)
- [ ] (s1) Bounded reads on HTTP responses (Prion #3)
- [ ] (s1) Stale-config detection/migration in the config crate (Animus H20)
- [ ] (s1/s2) Circuit-breaker compaction in the context manager (Fev)
- [ ] (s1) First real Provider backend + L0 smoke E2E (one real-GGUF run → valid trace + correct file edit)
