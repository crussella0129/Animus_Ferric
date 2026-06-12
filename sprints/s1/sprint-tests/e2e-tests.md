# Sprint 1 E2E Tests — Results

**Status: POSSIBLE for the first time — and PASSED.**

### `l0_smoke` (crates/ferric-cli/tests/l0_smoke.rs; the ADR-009 real-GGUF gate)
- Model: Llama-3.2-1B-Instruct Q4_K_M (771 MB) from `~/.animus/models`, CPU, deterministic sampler (temperature 0), release profile.
- Result: **PASS — all 8 assertions.** Exit 0; `hello.txt` == "hello ferric"; one parseable trace, v==1, seq monotonic from 0; session_start → session_end(final_text); turn pairs with output_tokens > 0; write_file call + non-error result + allow permission_check traced; offered tools include write_file and task_complete; 3 turns ≤ 15 budget.
- **Measured actuals** (research §4 unknown closed): 3 turns, 223 output tokens, **116.9 s wall including model load**, ~2.08 GB RSS while resident.
- **Finding 1 — debug-profile inference is unusable:** the first attempt spawned the debug binary at ~1 tok/s (single turn > 37 min, killed). `--release` is now mandated in the test docs. The flush-per-event trace made live diagnosis possible: the run was provably mid-turn-0, not hung.
- **Finding 2 — observed 1B behavior matches the lineage:** the model echoed a tool descriptor as text on one turn and *described* its `task_complete` call in prose on the final turn instead of calling it (hence `final_text`, not `task_complete`). The real write_file call executed correctly. Exactly the failure family the L0–L6 ladder and the s2 unified action grammar target.

### Still not possible in s1
- aarch64 runtime verification (gate is check-only; Pi/Orin deferred).
- L1–L6 quality benchmarks / tier calibration (harness ports in s2).
- HTTP escape-valve backend, streaming, GPU features.
- 7B quality run: not attempted this sprint (best-effort manual, not a gate).
