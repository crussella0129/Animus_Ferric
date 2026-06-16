# Sprint 2 Failure Report

> Exit artifact: s2 is classified **failed** — the headline deliverable (the unified
> action grammar running on a real model) did not land, and the cheap fix was
> disproven empirically. This report is the primary research input for s3.
> Note: the sprint's *other* deliverables shipped and are model-free-verified
> (112 tests) and CI-green; see test-report.md / unit-tests.md / integration-tests.md.
> "Failed" reflects the unmet headline goal honestly, not a total loss.

## What Failed
The real-GGUF E2E gate (ADR-009), both variants:
1. **`l0_smoke_grammar` — HANG.** `mistralrs::send_chat_request` with a `Constraint::JsonSchema` never returns on the GGUF-loaded Llama-3.2-1B; the trace freezes at `constraint_applied` (before any token), ~20 cores pegged, killed at 4 h. A bounded `grammar_probe` reproduced the hang with even a TRIVIAL one-field schema.
2. **`l0_smoke_native` — `repetition_guard`.** Native mode ran fine (54 s) but the 1B wrote the file correctly then looped on `write_file` instead of calling `task_complete`; the guard stopped it. No clean terminator → gate fails (correctly; the gate is narrow by design and was NOT loosened).

## Root Cause
Two distinct causes, both real-model, neither in Ferric's own logic (all model-free tests pass — loop, schema generation, action parsing, dispatch, guards, trace, bench harness):

1. **Grammar hang = mistral.rs 0.8.1 GGUF tokenizer synthesis.** Loading a GGUF without an explicit tokenizer.json makes mistral.rs synthesize one from GGUF metadata; llguidance builds its toktrie from that synthesized (byte-inconsistent) vocab, and the first mask traversal on the engine thread never returns. The unconstrained path never builds the toktrie → native works. Source-traced (gguf.rs / llg.rs / sampling.rs; closest upstream issue #2204, open). **The cheapest fix — supplying the authentic tokenizer.json via `with_tokenizer_json` — was implemented and DISPROVEN: the trivial probe still hangs with the real Llama-3.2 tokenizer confirmed loaded.** So either `with_tokenizer_json` doesn't rewire the llguidance factory on the GGUF pipeline, or the hang is not the synthesized tokenizer.

2. **Native non-termination = model capability floor.** Sub-7B models (Llama-3.2-1B) are below the reliable tool-calling/termination threshold regardless of harness (research-corroborated). Grammar would FORCE clean termination (task_complete as a grammar-required branch) — which is exactly why cause #1 blocking grammar is what makes cause #2 visible.

## Required Re-architecture
s3 must, in order of increasing cost:
1. **Add a hard per-request inference timeout + standalone-`query` wall-clock kill** (independent of grammar — no engine pathology may ever run unbounded again; only `ferric bench` had a timeout in s2).
2. **Grammar-enablement spike:** cheap in-process attempts first (`with_tok_model_id`; mistral.rs version bump). If in-process stays dead, choose: llama-server HTTP backend (grammar server-side in llama.cpp — known-good, but sacrifices the in-process 100%-Rust ownership chain) OR a Candle+llguidance in-process toktrie built by us (preserves purity, hardest, same GGUF-toktrie hazard). This is an architectural fork for the human at s3 research.
3. **Capability tier:** adopt a tool-tuned ≥7–12B model — **test Gemma 4 12B (user lead, reportedly strong with harnesses)** and Qwen2.5-Coder-7B (already in the fleet) as the primary tier; keep the 1B as the cheap NANO/CI gate. The native path works today, so capability evaluation can proceed before grammar is fixed.

## State at Failure
- Completed (build phase, all committed + model-free-verified + CI-green): T-201..T-216 — ferric-prompt (oovra composition), the unified action grammar (schema gen + parse + loop integration), ActionProtocol, move_path/make_dir, output-token budgets, PolicySelected/PromptComposed trace events, the full ferric-bench L0–L6 harness + calibration, the `ferric query`/`ferric bench` surfaces, ADR-015..020.
- Test-phase additions: grammar_probe repro (feature-gated #[ignore]); tokenizer.json backend plumbing (kept, doesn't fix the hang); select_protocol default flipped to NativeTools (ADR-020).
- Failed in: the real-GGUF E2E gate (both protocol variants).
- Not started / deferred to s3: grammar-enablement fix, the inference timeout safety net, the Gemma-4-12B / Qwen-7B capability evaluation, the first real L0–L6 calibration sweep.
