Finalized - DO NOT EDIT

# Sprint 30 Build Plan — Ornstein quarantined summarizer (`ferric-research`, increment 1)

Pivot to hardening Animus Loop. Build Ornstein's heart — a quarantined summarizer that turns
untrusted content into typed, provenance-tagged data via Ferric's constrained valve (empty
tools + a data-only JSON schema + single-shot). New `ferric-research` crate. Container/proxy/
CaMeL-sink/Loop-wiring deferred. Rationale: `sprints/s30/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the quarantined summarizer, as a crate, tested + recorded.
  - **A. crate + types** — T-3001
  - **B. the quarantine** — T-3002
  - **C. ADR + docs** — T-3003

## Execution Sequence

### T-3001: Scaffold `ferric-research` + `ResearchDigest` + schema
- **Touches:** `Cargo.toml` (workspace members + dep), `crates/ferric-research/Cargo.toml` (new), `crates/ferric-research/src/lib.rs` (new)
- **Depends on:** —
- **Description:** new workspace crate (deps: ferric-core, ferric-provider, serde, serde_json, thiserror; dev: futures-executor). `ResearchDigest { source: String, untrusted: bool, summary: String, claims: Vec<Claim> }`, `Claim { claim: String, quote: String }` (serde, data-only). `digest_schema() -> serde_json::Value`.
- **Success (EARS):** WHEN `ResearchDigest`/`Claim` are serialized THEN they SHALL contain only data fields (source, summary, claim, quote, untrusted) and no field that can express a tool/action. WHEN the crate is added THEN `cargo build --workspace` SHALL include it.

### T-3002: `summarize_quarantined` — the quarantine
- **Touches:** `crates/ferric-research/src/lib.rs`
- **Depends on:** T-3001
- **Description:** `async fn summarize_quarantined(provider: &dyn Provider, source: &str, untrusted_content: &str, question: &str) -> Result<ResearchDigest, ResearchError>`. Build a single-shot `CompletionRequest` (system + one user message; the content fenced as untrusted data) with **empty tools** + `Some(Constraint::JsonSchema(digest_schema()))`; `provider.complete`; parse `message.text` into `ResearchDigest`; **stamp** `source` + `untrusted = true`. `ResearchError` (thiserror) for parse/provider failures.
- **Success (EARS):**
  - WHEN called THEN it SHALL issue exactly one completion whose `tools` is empty and `constraint` is `Some(JsonSchema(_))`.
  - WHEN the model returns a valid digest THEN it SHALL parse it and overwrite `source` + set `untrusted = true` (harness-stamped provenance).
  - WHEN the untrusted content carries an injection THEN the result SHALL remain a pure `ResearchDigest` (injection text only ever in a `quote`; no action channel exists).
  - WHEN the model output is malformed THEN it SHALL return `ResearchError`, not panic.

## Post-build (test)
- `cargo test -p ferric-research` (new unit tests incl. injection containment) + `cargo test --workspace` green; clippy `-D warnings`; fmt.

### T-3003: ADR-040 + docs
- **Touches:** `decisions.md`, `docs/ornstein.md` (new), `README.md`, `agent-tasks/*`
- **Depends on:** T-3002
- **Description:** ADR-040 (Ornstein recovered; increment-1 quarantined summarizer = the constrained valve as a security primitive; the lethal-trifecta frame; the deferred layers; s1 source pointer). `docs/ornstein.md` quickstart. README Status 30 + Sprint 30 timeline.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-040 and README SHALL show Sprint 30.
