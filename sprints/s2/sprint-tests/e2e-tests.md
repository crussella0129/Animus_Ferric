# Sprint 2 E2E Tests — Results

**Status: POSSIBLE. Both real-GGUF gates FAILED — and both failures are the kind ADR-009 exists to surface. Neither is a defect in Ferric's own logic (all 112 model-free tests pass); both are real-model/engine findings that drove design decisions.**

Model: Llama-3.2-1B-Instruct Q4_K_M, CPU, release, temperature 0.

## Finding 1 — `l0_smoke_grammar` HANGS (ADR-020)
`mistralrs::send_chat_request` with a `Constraint::JsonSchema` never returned: the trace froze exactly at `constraint_applied` (the event immediately before the inference call), no `turn_end`, ~20 cores pegged for **4+ hours**, RSS climbing to 3.5 GB, killed manually. The standalone `ferric query` had no wall-clock kill (only `ferric bench` did), so it ran unbounded.
- **Not Ferric's logic:** every model-free grammar test passes (MockProvider bypasses the real engine) — schema generation, action parsing, dispatch, and trace are all correct. The pathology is downstream in llguidance grammar compilation or per-token masking over the generated schema on the GGUF tokenizer.
- **Decision (ADR-020):** `select_protocol` auto-default flipped to `NativeTools`; UnifiedGrammar is opt-in via `--protocol grammar`. `ferric query` can no longer hang by default. s3 tasks: minimal repro + schema-feature bisect; hard per-request inference timeout; standalone-query wall-clock kill; re-enable grammar default only after a green re-run.
- **ROOT CAUSE NAILED (grammar_probe, bounded 300s subprocesses):** even a TRIVIAL schema `{type:object, properties:{x:string}, required:[x]}` hangs — model loads in 4.6s, then `complete()` with any `Constraint::JsonSchema` never returns (killed at the cap, no token emitted). This eliminates every schema hypothesis (anyOf breadth, unbounded strings, x-guidance). The defect is mistralrs 0.8.1's llguidance constraint enforcement on a **GGUF-loaded** model — the grammar/toktrie setup over the GGUF-derived tokenizer hangs before generation. **Not fixable from Ferric's side** by schema or loop changes. Fix requires an upstream path: a mistralrs version bump (gamble; 0.8.3 needs a candle git rev), an explicit `tokenizer.json` to feed llguidance's toktrie (untested workaround), or grammar via the llama-server HTTP backend (server-side json_schema→GBNF, bypasses mistralrs llguidance entirely — pulls ADR-017 forward). The `grammar_probe` test is retained (feature-gated, #[ignore]) as the repro.

## Finding 2 — `l0_smoke_native` ends in `repetition_guard` (1B capability limit; NOT a budget bug, NOT a Ferric defect)
Native mode ran cleanly (54 s, 4 turns, no hang); terminator `repetition_guard`, not a clean `task_complete`/`final_text`. **Trace-confirmed root cause** (initial token-budget hypothesis DISPROVEN — output was 27/32/32/32 tokens, far under the 512 cap, so the NANO budget is vindicated):
- turn 0: `write_file(hello.txt, "hello ferric.")` → 13 bytes (stray period)
- turn 1: `write_file(hello.txt, "hello ferric")` → 12 bytes, **correct**
- turns 2–3: identical `write_file` → guard `warned` then `stopped`
The 1B completed the actual task (hello.txt = "hello ferric") but **never called `task_complete`** — it looped on `write_file`, and the hash-all repetition guard correctly caught it. This is the same small-model terminator failure s1 saw (there the 1B narrated → `final_text`, passing by luck; here it loops → guard stop). It is the measured-capability finding the L0–L6 ladder exists to capture, and it directly motivates the unified grammar (which would FORCE `task_complete` as a grammar-required action) — which is blocked by Finding 1.
- Every Ferric component worked correctly in the trace: policy_selected, prompt_assembled (5 tools + task_complete), guarded tool dispatch with allow checks, hash-all repetition guard firing exactly on the duplicate set, full flush-per-event trace. The pipeline is verified end-to-end on a real model; the model is simply borderline at L0's clean-termination bar.
- The smoke gate is correctly NARROW (C-004): it refused to green a run where the model never cleanly terminated. The gate working, not a flaky assertion.
- (Minor observed quirk: mistralrs passed a spurious `"type":"object"` arg into `write_file` on turns 1–3; our tool ignored it and wrote correctly. Noted, harmless.)
- Budget NOT changed (512 disproven as the cause). Calibration item for s3: stronger post-write terminator nudge for NANO native, or rely on the unified grammar once Finding 1 is fixed.

## Model-free coverage (the sprint's logic IS verified)
112 default-feature tests pass: the full UnifiedGrammar loop (grammar_loop, truncation_tests), native regression, bench harness end-to-end (bench_mock spawn-self), schema golden, calibration. The grammar and the loop are proven correct in isolation; only the real-engine enablement (Finding 1) and the budget seed (Finding 2) need follow-up.

## Calibration sweep
Deferred: blocked on Finding 2 (native must produce clean terminators before a sweep is meaningful) and Finding 1 (grammar disabled). The harness itself is verified by bench_mock; the first real sweep runs once the NANO budget is re-seeded and native L0 passes.
