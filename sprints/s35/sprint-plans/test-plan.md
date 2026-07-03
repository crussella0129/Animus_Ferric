Finalized - DO NOT EDIT

# Sprint 35 Test Plan — Expert review + refactor

## Unit — `ferric-guard` (T-3501)
- **Read denials:** `.ssh/id_rsa`, `.aws/credentials`, a bare `.env` at workspace root,
  `.gnupg/anything`, `.kube/config` → `Decision::Deny` on `PermissionLevel::Read`.
- **Regression — reads NOT newly broken:** `.git/config` read → still `Allow` (git metadata
  reads are a legitimate agent need — only writes to `.git` are denied); an ordinary file read
  (e.g. `src/main.rs`) → still `Allow`.
- **Write behavior unchanged:** existing write-denial tests (`.git/config`, `.ssh/id_rsa`, stray
  `id_ed25519`) continue to pass unmodified — T-3501 only touches the `Read` arm.

## Unit — `ferric-cli::server` (T-3502)
- **Flags present → argv includes them:** `ServerConfig{threads: Some(4), gpu_layers: Some(20),
  batch_size: Some(512), ..}` on `Engine::LlamaServer` → argv contains `-t 4`, `-ngl 20`, `-b 512`.
- **Flags absent → unchanged (backward compat):** all three `None` → argv byte-identical to the
  existing `llama_server_argv` test's expected output.
- **Ollama unaffected:** the same `threads`/`gpu_layers`/`batch_size` set on an `Engine::Ollama`
  config → argv is unchanged (`serve` only) — these fields are accepted but not passed through.

## Build/Lint — `mistralrs` pin (T-3503)
- `cargo build -p ferric-cli --features backend-mistralrs` succeeds after the `rev` pin (manual
  check — a Cargo.toml/Cargo.lock change, no new unit test surface).

## Build/Lint + regression — `reqwest` TLS (T-3504)
- `cargo test --workspace` green after the feature swap (or no swap, if evaluation shows it's
  unnecessary — recorded either way in ADR-045).
- Existing `ferric-provider` HTTP-path tests (mock-based) remain green — TLS backend choice
  doesn't change request/response shape.
- If a live llama-server is reachable, one smoke `ferric query` over `--backend openai` confirms
  the loopback HTTP path still functions post-swap.

## Build / Lint (default CI, all tasks)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean;
  `fmt --check` clean.

## E2E
- Not required beyond the T-3504 optional live smoke: T-3501/3502 are pure unit-testable logic;
  T-3503 is a dependency-resolution change verified by a successful build.
