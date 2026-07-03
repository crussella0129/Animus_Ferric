Finalized - DO NOT EDIT

# Sprint 35 Build Plan — Expert review + refactor

A full-project audit (security/efficiency/product-completeness) executed as a contained refactor:
four small, independent, immediately-effective fixes, plus an ADR recording the full audit and an
explicit, reasoned deferral list. Rationale: `sprints/s35/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** four audit-driven fixes shipped, tested, and recorded.
  - **A. read-side sensitive-file guard** — T-3501
  - **B. server edge-tuning flags** — T-3502
  - **C. mistralrs rev-pin** — T-3503
  - **D. reqwest TLS evaluation/swap** — T-3504
  - **E. ADR + docs** — T-3505

## Execution Sequence

### T-3501: Read-side sensitive-file guard
- **Touches:** `crates/ferric-guard/src/denylist.rs`, `crates/ferric-guard/src/checker.rs`
- **Depends on:** —
- **Description:** `DENIED_READ_SEGMENTS = [".ssh",".gnupg",".aws",".kube",".ferric"]` (write segments minus `.git`); `DENIED_READ_FILES = DENIED_WRITE_FILES + [".env"]`. `check()`'s `PermissionLevel::Read` arm calls a new `check_read_target(path)` (mirrors `check_write_target`) instead of unconditional `Allow`.
- **Success (EARS):** WHEN a read targets a credential-store segment or known-secret filename THEN it SHALL be denied. WHEN a read targets `.git/config` or an ordinary path THEN it SHALL remain Allowed.

### T-3502: `ferric server` edge-tuning flags
- **Touches:** `crates/ferric-cli/src/server.rs`
- **Depends on:** —
- **Description:** add `threads`/`gpu_layers`/`batch_size: Option<u32>` to `ServerUpArgs` (`--threads`/`--gpu-layers`/`--batch-size`) and `ServerConfig`; wire through the `up` construction path; `command()`'s `LlamaServer` branch pushes `-t`/`-ngl`/`-b` when set; `Ollama` branch ignores them.
- **Success (EARS):** WHEN the new flags are supplied with `--engine llama-server` THEN argv SHALL include the matching flags. WHEN omitted THEN argv SHALL be byte-identical to today.

### T-3503: Pin `mistralrs` to a specific commit
- **Touches:** `Cargo.toml`
- **Depends on:** —
- **Description:** resolve the current `EricLBuehler/mistral.rs` master HEAD commit (`git ls-remote`); replace `branch = "master"` with `rev = "<sha>"`, comment matching oovra's reproducibility policy.
- **Success (EARS):** WHEN the workspace resolves `mistralrs` THEN it SHALL use a fixed commit. WHEN `--features backend-mistralrs` builds THEN behavior SHALL be unchanged.

### T-3504: Evaluate + likely switch `reqwest` to `rustls-tls`
- **Touches:** `Cargo.toml`
- **Depends on:** —
- **Description:** check reqwest's resolved TLS backend; if native-tls/OpenSSL, switch to `default-features = false, features = ["json","rustls-tls"]`.
- **Success (EARS):** WHEN built THEN the HTTP provider SHALL not require native OpenSSL. WHEN existing HTTP-provider tests run THEN they SHALL remain green.

### T-3505: ADR-045 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-3501, T-3502, T-3503, T-3504
- **Description:** ADR-045 (audit summary + link to research report; the four remediations; the explicit, reasoned deferral list — sink-policy wiring, MCP/chat, shell/git tools, streaming, session resume, trace rotation). README Status 35 + Sprint 35 timeline.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-045 and README SHALL show Sprint 35.

## Post-build (test)
- `cargo test -p ferric-guard -p ferric-cli` (new tests) + `cargo test --workspace` green; clippy `-D warnings`; fmt.
