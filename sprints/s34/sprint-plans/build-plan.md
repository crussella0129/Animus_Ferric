Finalized - DO NOT EDIT

# Sprint 34 Build Plan — Ornstein: the CaMeL-lite sink-policy primitive

CaMeL-lite flow control on top of the quarantine: taint tracking + a configurable sink policy so
tainted (untrusted research) data can't reach a side-effecting sink unchecked. Pure primitive,
all three enforcement modes (caller picks), no loop wiring yet. In `ferric-research`. Rationale:
`sprints/s34/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the sink-policy + taint primitive, tested + recorded.
  - **A. sink.rs (SinkPolicy + TaintSet)** — T-3401
  - **B. ADR + docs** — T-3402

## Execution Sequence

### T-3401: `sink.rs` — `SinkPolicy` + `TaintSet`
- **Touches:** `crates/ferric-research/Cargo.toml` (add `ferric-guard`), `crates/ferric-research/src/sink.rs` (new), `crates/ferric-research/src/lib.rs`
- **Depends on:** —
- **Description:** `SinkAction { Deny, RequireApproval, Warn }`; `SinkDecision { Allow, Deny, RequireApproval, Warn }`; `SinkPolicy { tainted_sink }` (`new`, `deny`, `decide(permission, tainted) -> SinkDecision`); `TaintSet { tainted: Vec<String> }` (`taint_str`, `taint_digest(&ResearchDigest)`, `is_tainted(&str)`, `args_tainted(&serde_json::Value)`). Re-export.
- **Success (EARS):**
  - WHEN args are not tainted THEN `decide` SHALL return `Allow` for any permission.
  - WHEN args are tainted AND permission is `Read` THEN `decide` SHALL return `Allow`.
  - WHEN args are tainted AND permission is `Write`/`Execute` THEN `decide` SHALL return the `SinkDecision` matching the policy's `SinkAction`.
  - WHEN a digest is tainted THEN `args_tainted` SHALL flag a tool-arg JSON containing one of its quotes/summary, and not a clean one.

### T-3402: ADR-044 + docs
- **Touches:** `decisions.md`, `docs/ornstein.md`, `README.md`, `agent-tasks/*`
- **Depends on:** T-3401
- **Description:** ADR-044 (CaMeL-lite primitive; substring taint + policy matrix keyed off `PermissionLevel`; all-three-modes; enforcement deferred to the dispatch chokepoint with the loop wiring; the conservative-substring tradeoff). docs/ornstein.md CaMeL section; README Status 34.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-044 and README SHALL show Sprint 34.

## Post-build (test)
- `cargo test -p ferric-research` (new sink tests + the existing 21) + `cargo test --workspace` green; clippy `-D warnings`; fmt.
