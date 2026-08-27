Finalized - DO NOT EDIT

# Sprint 22 Test Plan — Sharper repetition nudge for the 1B

## Unit (`ferric-loop`, default CI)
- `repetition_tests`: on the 3rd identical turn the nudge reaches the model and now contains **`task_complete`** (the stable directive); the guard still yields `["warned","stopped"]` and stops with `StopReason::RepetitionGuard`. The two-strike behavior is unchanged — wording only.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean.

## End-to-End — RUN it (the measurement)
ollama up; binary built `--features backend-openai`:
```
ferric bench --backend openai --api-base http://localhost:11434/v1 \
  --model llama3.2:1b --params-b 1 --protocol grammar --results-dir benchmarks
```
- **Headline:** does the sharper nudge lift `llama3.2:1b` above s21's `measured_level: none`? Compare per-level PASS/FAIL — especially **L0** (the repeat-not-terminate case).
- Both outcomes are valid and recorded honestly:
  - **Improved** (clears L0+) → the nudge wording was the bottleneck; ship it, the 1B's agentic floor rose.
  - **Still none** → the 1B's ceiling is deeper than wording; ADR-031 documents the limitation with the trace evidence (the nudge still can't hurt larger models, which already terminate).

## Notes
- The loop unit test is the AI-verifiable core; the live 1B re-bench is the measurement that decides the ADR's conclusion.
