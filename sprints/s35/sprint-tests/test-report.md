# Sprint 35 Test Report — Expert review + refactor (ADR-045)

**Date:** 2026-06-29. The first full-project audit sprint: security/efficiency/product-
completeness, executed as four small, immediately-effective fixes. All tests green across
default and `backend-mistralrs` feature sets.

## Build / Lint (green)
- `cargo test --workspace` green (default features).
- `cargo build -p ferric-provider --features backend-mistralrs` succeeds at the pinned rev
  (T-3503) — resolves and compiles cleanly against the fixed commit.
- `cargo clippy --workspace --all-targets -- -D warnings` clean (default features) **and**
  `cargo clippy -p ferric-provider --all-targets --features backend-mistralrs -- -D warnings`
  clean (the feature-gated path, not just the default build).
- `cargo fmt --all --check` clean.

## Unit — `ferric-guard` (T-3501) — 5 new / 13 in the crate, all pass
- `denies_reading_credential_store_segments`: `.ssh/id_rsa`, `.gnupg/secring.gpg`,
  `.aws/credentials`, `.kube/config` all denied on `Read`.
- `denies_reading_ferric_trace_dir`: `.ferric/trace/x.jsonl` read denied (symmetric with the
  write-side self-protection rule).
- `denies_reading_dotenv`: a bare `.env` read denied; **write to the same path stays Allowed**
  (the read/write asymmetry is deliberate and now tested).
- `denies_reading_stray_private_key`: `backup/id_ed25519` (filename rule outside a denied segment)
  denied on read.
- **Regression — `allows_plain_read`** (existing test, updated comment only): `.git/config` read
  and an ordinary file read both remain `Allow` — proves the new guard doesn't overreach into
  legitimate git-metadata/code-context reads.
- All existing write-denial tests (`denies_sensitive_paths`, `allows_ordinary_write`,
  `denylist_is_const`) pass unmodified.

## Unit — `ferric-cli::server` (T-3502) — 2 new / 23 in the crate, all pass
- `llama_server_edge_tuning_flags`: `threads=Some(4), gpu_layers=Some(20), batch_size=Some(512)`
  on `Engine::LlamaServer` → argv contains `-t 4`, `-ngl 20`, `-b 512`.
- `ollama_ignores_edge_tuning_flags`: the same fields set on `Engine::Ollama` → argv unchanged
  (`["serve"]`) — confirms the fields are accepted but not passed through.
- **Regression — `llama_server_argv`** (existing test, unmodified): all three fields `None` →
  argv byte-identical to pre-sprint output — proves backward compatibility for free.

## Dependency changes (T-3503, T-3504) — verified by build + tree inspection
- `mistralrs`: `git ls-remote` resolved current master HEAD
  (`15986c037bbe3ee31085d1c73abd2ea3cb11f094`); pinned via `rev`; `cargo build --features
  backend-mistralrs` succeeds at the pin.
- `reqwest`: `cargo tree -p ferric-provider -e features -i reqwest` before the change showed
  `default-tls` active; after `default-features = false, features = ["json", "rustls-tls"]`, the
  same command shows `rustls-tls`/`__rustls` and no `default-tls`. `cargo test --workspace`
  remains green post-swap.

## Panic-safety sub-audit (not a new test suite — a targeted grep + manual verification)
Grepped `\.unwrap\(\)|\.expect\(|panic!` across `ferric-loop/src`, `ferric-provider/src`, and
`ferric-tools/src/builtin` (the surfaces touching model output, backend responses, and file
content). Result: **clean**. The two live-path hits in `ferric-loop/src/grammar.rs` (regex
capture-group access) are structurally guaranteed safe (the groups are non-optional in the
pattern); every other hit was confirmed inside `#[cfg(test)]` by reading the surrounding module.
Zero hits in any builtin tool. No DoS-via-panic vector found.

## Verdict
**All four remediations validated**, each backward compatible or intentionally scoped (the
read/write asymmetry on `.env`, the git-metadata-reads-stay-legitimate carve-out). The full audit
(research report + ADR-045) also honestly records what's clean (panic safety, the mistralrs/tokio
default-off feature gating, the workspace boundary logic) alongside what's deferred and why. No
human-verification checkpoint required — every claim here is either a passing automated test or a
directly-inspected build/dependency-tree fact. ADR-045.
