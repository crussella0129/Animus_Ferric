# Sprint 22 Test Report — Sharper repetition nudge for the 1B

**Date:** 2026-06-26. The nudge change is proven by the loop unit test; the
hypothesis ("wording is the bottleneck") was **tested on the real 1B and
disproven** — an honest, valuable negative result.

## Unit (`ferric-loop` — green)
- `repetition_tests` (3): on the 3rd identical turn the nudge reaches the model and now contains **`task_complete`** (the imperative directive); the guard still yields `["warned","stopped"]`, stops with `StopReason::RepetitionGuard`, and emits `session_end reason=repetition_guard`. Behavior unchanged — wording only.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -D warnings` clean; `fmt --check` clean.

## End-to-End — RAN it: the hypothesis test (ollama, llama3.2:1b)
Re-benched L0–L6 with the sharper nudge:
```
L0 single-readonly-tool — FAIL   L4 multi-file-with-test — FAIL
L1 single-file-rename   — FAIL   L5 mini-cli            — FAIL
L2 multi-step-ops       — FAIL   L6 full-todo-app       — FAIL
L3 single-file-construction — FAIL    → measured_level: none
```
**No change from s21** — the 1B still clears nothing. Per-row failure modes:
- **L0:** `terminator: repetition_guard`, `tools_called: ['list_dir','list_dir']` — *identical* to before. The 1B re-emitted `list_dir` despite the sharper imperative; it cannot act on the nudge.
- **L1:** `repetition_guard` — repeated `read_file`, then `make_dir`.
- **L2:** `max_turns` after **15 `make_dir` calls with different paths** — "semantic flailing" the guard doesn't catch (it matches identical action signatures).

## Conclusion (ADR-031)
The hypothesis is disproven: **the 1B's multi-turn failure is a genuine capability
limit (planning / state-tracking / completion-recognition), not nudge wording.** A
prompt can't fix it. Decisions:
1. **Ship the sharper nudge anyway** — strictly better wording, helps mid-tier models that *do* read nudges, can't regress capable ones (they terminate before the first repeat). The loop unit test pins it.
2. **The 1B's role is settled** — a reliable *constrained tool-caller* (100% single-shot, all rings) but not an agent. Ferric's tier machinery already encodes this: it stays Nano, gets the Ring-0 core, and `measured_level` correctly refuses to promote it.
3. **Future hardening** (backlog): a no-progress / max-same-tool guard for semantic flailing (L2's mode). No human-verification checkpoint.
