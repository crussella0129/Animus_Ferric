# Agent Tasks (Persistent Backlog)

- [ ] T-107 (sprint 1): exponential backoff on retryable errors — touches: crates/ferric-loop/src/{backoff.rs,run.rs}
- [ ] T-108 (sprint 1): workspace deps + backend-mistralrs feature + CI backend-check job — touches: Cargo.toml, crates/ferric-{provider,cli}/Cargo.toml, .github/workflows/ci.yml
- [ ] T-109 (sprint 1): MistralRsProvider (feature-gated) — touches: crates/ferric-provider/src/mistralrs.rs, lib.rs
- [ ] T-110 (sprint 1): CLI graduates to clap (trace cat byte-identical) — touches: crates/ferric-cli/src/*
- [ ] T-111 (sprint 1): ferric query subcommand (--mock + real path) — touches: crates/ferric-cli/src/{main.rs,query.rs}
- [ ] T-112 (sprint 1): L0 smoke E2E (#[ignore], feature-gated, env-driven) — touches: crates/ferric-cli/tests/l0_smoke.rs
- [ ] T-113 (sprint 1): ADR-010..014 + backlog roadmap rewrite — touches: decisions.md, agent-tasks/

## Deferred lineage fixes (from sprint 0 Lineage-Fix Ledger — must not evaporate)
- [ ] (s1) Hash-ALL-calls repetition guard in the agent loop (Prion #5)
- [ ] (s1) Structured terminator (task_complete) wired into constraint grammar (Animus)
- [ ] (s1) Exponential backoff on retryable provider errors (Prion #6)
- [ ] (s1) Bounded reads on HTTP responses (Prion #3)
- [ ] (s1) Stale-config detection/migration in the config crate (Animus H20)
- [ ] (s1/s2) Circuit-breaker compaction in the context manager (Fev)
- [ ] (s1) First real Provider backend + L0 smoke E2E (one real-GGUF run → valid trace + correct file edit)
