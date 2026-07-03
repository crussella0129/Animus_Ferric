Finalized - DO NOT EDIT

# Sprint 34 Test Plan — Ornstein CaMeL-lite sink policy

## Unit — `sink.rs` (`ferric-research`; pure, deterministic)
- **`decide` — untainted:** `decide(Read|Write|Execute, false)` → `Allow` (trusted data always flows).
- **`decide` — Read + tainted:** `decide(Read, true)` → `Allow` (reading isn't a dangerous sink).
- **`decide` — Write/Execute + tainted, all modes:** `SinkPolicy::new(Deny).decide(Write, true)` →
  `Deny`; `RequireApproval` → `RequireApproval`; `Warn` → `Warn`; `Execute` behaves like `Write`.
- **`TaintSet` substring:** `taint_str("secret-token")`; `is_tainted("...secret-token...")` true;
  `is_tainted("clean")` false; an **empty** set → `is_tainted` / `args_tainted` always false (empty
  tainted strings never match).
- **`taint_digest`:** taints a `ResearchDigest`'s `summary` + each `claim.quote`; `is_tainted` on a
  value containing a quote → true.
- **`args_tainted` walks JSON:** a tainted substring nested in an object value → true; nested in an
  array element → true; a clean args object → false.
- **end-to-end shape (the proof):** taint a digest whose quote is an injection; build a
  `write_file` args JSON echoing that quote; `args_tainted(args)` true → `SinkPolicy::deny().decide(
  Write, true)` → `SinkDecision::Deny` — the gate the wiring will enforce on an injected write.

## Build / Lint (default CI)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean;
  `fmt --check` clean. `ferric-guard` added to `ferric-research` (workspace crate; no cycle).

## E2E
- Not required: a pure decision function. The live gate (taint set populated as digests enter
  context; `decide` enforced at the dispatch chokepoint) arrives with the research→loop wiring.
