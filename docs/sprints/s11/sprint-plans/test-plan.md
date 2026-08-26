Finalized - DO NOT EDIT

# Sprint 11 Test Plan — Re-enable constrained decoding on mistral.rs

## Unit Tests (feature `backend-mistralrs`)
- **T-1101** (`mistralrs.rs`): `to_mistralrs_constraint` maps each variant to the matching `mistralrs::Constraint` — `JsonSchema(v)`→`JsonSchema`, `Lark(s)`→`Lark`, `Regex(s)`→`Regex`, asserted via `matches!` (the mistralrs type may not impl `PartialEq`).

## Build / Lint
- `cargo build -p ferric-provider --features backend-mistralrs` clean; `cargo clippy -p ferric-provider --features backend-mistralrs -- -D warnings` clean.
- `cargo test --workspace` (default) unaffected — the mapping is feature-gated.

## End-to-End — the empirical answer (RUN it, bounded)
Re-run `grammar_probe` through the now-wired provider on `D:\Models\Llama-3.2-1B-Instruct-Q4_K_M.gguf`, each variant its own `timeout`-bounded subprocess:
```
FERRIC_PROBE=trivial FERRIC_SMOKE_MODEL_DIR=D:/Models FERRIC_SMOKE_MODEL_FILE=Llama-3.2-1B-Instruct-Q4_K_M.gguf \
  timeout 240 cargo test -p ferric-provider --release --features backend-mistralrs --test grammar_probe -- --ignored --nocapture
# then FERRIC_PROBE=unified (the exact ADR-020 hang trigger)
```
**Classify the result:**
- **(a) enforces** — returns within the timeout AND the output now **matches the schema** (trivial → an object with `x`; unified → a `{tool,args}` object), NOT the old freeform "John Doe" JSON.
- **(b) ignores** — returns but still freeform (schema had no effect).
- **(c) hangs** — killed by `timeout` (ADR-020 recurs on 0.8.15).

Fully runnable now (GGUF present; mistralrs already compiled). No human setup.

## Loop / Decision (ADR-027)
- **(a):** flip `supports_constraint:true` → `select_protocol` routes mistral.rs to `ConstrainedJson`; a pure-Rust constrained backend. Update README backends/Status + timeline.
- **(b)/(c):** guard `set_constraint` off (keep the strip; no regression); ADR-027 records the definitive in-process verdict — HTTP valve stays the constrained workhorse.
