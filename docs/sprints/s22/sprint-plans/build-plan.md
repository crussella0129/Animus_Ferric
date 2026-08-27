Finalized - DO NOT EDIT

# Sprint 22 Build Plan — Sharper repetition nudge for the 1B

Diagnosis (kept trace): the 1B fails L0 by **repeat-not-terminate** — it calls
`list_dir` twice instead of `task_complete`, hitting the repetition guard. The
existing nudge is soft/conditional and the 1B ignored it. Make it a direct
imperative naming the repeated tool; re-bench. First sprint under one-PR-per-sprint.
Rationale: `sprints/s22/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** help the 1B transition to task_complete instead of repeating.
  - **A. sharper nudge** — T-2201
  - **B. re-bench + ADR/docs** — T-2202

## Execution Sequence

### T-2201: Sharpen the repetition nudge
- **Touches:** `crates/ferric-loop/src/run.rs`, `crates/ferric-loop/tests/repetition_tests.rs`
- **Success (EARS):** the `Verdict::Warn` nudge is a direct imperative naming the repeated tool(s) — "You already called `<tool>` and have the result — do not call it again. If the task is finished, call task_complete now with a one-sentence summary." Two-strike behavior unchanged.
- **Tests:** `repetition_tests.rs` asserts the nudge contains `task_complete` (was `repeating`); `["warned","stopped"]` unchanged.

### T-2202: Re-bench the 1B + ADR + docs
- **Touches:** `decisions.md`, `README.md`
- **Success (EARS):** re-bench `llama3.2:1b` L0–L6 → record measured_level vs s21's `none`; ADR-031 (repeat-not-terminate mechanism + mitigation effect); README Status 22 + Sprint 22 timeline.

## Post-build (test)
- `repetition_tests` + workspace green; the live 1B re-bench (does it clear L0 now?).

## Loop close (workflow under test)
- One PR per sprint: after close+push `dev`, `gh pr create --base main --head dev` titled `Sprint 22 — …` (diff = only s22), then schedule next.
