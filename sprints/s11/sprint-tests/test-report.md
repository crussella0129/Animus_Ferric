# Sprint 11 Test Report — mistral.rs constrained decoding: definitively HANGS on 0.8.15

**Date:** 2026-06-24 · **Model:** `D:\Models\Llama-3.2-1B-Instruct-Q4_K_M.gguf` · mistralrs 0.8.15 (git `80fdfbc`).

## Unit (build, green)
- **T-1101** `constraint_maps_to_mistralrs_variants` — `to_mistralrs_constraint` mapped `JsonSchema/Lark/Regex` 1:1; `cargo clippy --features backend-mistralrs -D warnings` clean. (The mapping fn + test were removed in the revert below; they passed while present.)

## E2E — the empirical answer (RUN, bounded)
Re-ran `grammar_probe` through the now-wired `MistralRsProvider::complete()`:
```
FERRIC_PROBE=trivial … timeout 360 cargo test -p ferric-provider --release \
  --features backend-mistralrs --test grammar_probe -- --ignored --nocapture
```
**Result: (c) HANGS.** The probe ran **314 s** then **panicked** at `grammar_probe.rs:100` (`provider.complete(...).await.expect("complete")`) — `complete()` returned the **5-minute engine-timeout** error (`"inference timeout: engine hung for >5m"`). The trivial schema is just `{x: string, required:[x]}` — **the simplest possible JSON-Schema, and it still hangs.**

Contrast: the *stripped* path (pre-T-1101, ADR-025) returned the same input in **~10 s** with freeform output. So the only variable is whether the constraint reaches the engine — and when it does, llguidance/toktrie hangs on GGUF.

`unified` was **not** run: a trivial schema hanging is conclusive — a more complex schema cannot do better, and it would only cost another 5-minute timeout.

## Conclusion — the question is now definitively answered
- **The ADR-020 hang is NOT fixed in mistralrs 0.8.15.** ADR-025's "0.8.15 returns" was measuring the deliberately-*stripped* path (the provider never passed the constraint), not enforcement. With the constraint actually applied, the engine hangs even on a trivial schema.
- **mistral.rs cannot be a constrained pure-Rust backend on this version.** `supports_constraint` stays `false`; the loop keeps routing it to `TextXml`. The HTTP valve (llama.cpp/Ollama, ADR-001) remains the constrained workhorse — proven at 100% to 1B in sprint 9.

## Decision (ADR-027) — revert the wiring, no regression
T-1101's `set_constraint` wiring was **reverted** (the strip restored, now documented inline with this finding). Crucial reason: leaving it in would make `ferric toolbench --backend mistral --protocol grammar` (which builds a `Constraint::JsonSchema`) **5-minute-hang** on every tool — a real regression. Post-revert: `clippy --all-targets -D warnings` clean, 11 lib tests pass, `fmt` clean; default workspace untouched (the module is feature-gated).

**Sprint outcome:** the open question from ADR-025/026 is closed with a hard, reproducible result; the codebase is back to its safe state; future "try mistral.rs constraints again" only needs to re-check this when the upstream llguidance-on-GGUF hang is fixed.
