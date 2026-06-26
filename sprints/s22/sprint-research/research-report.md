# Sprint 22 Research Report — Why the 1B fails L0, and a sharper repetition nudge

> Sprint 21's headline: `llama3.2:1b` fires single tool calls at 100% (toolbench)
> but **fails even L0** as a multi-turn agent. This sprint diagnoses *why* and tries
> one bounded mitigation. (Also the first sprint under the **one-PR-per-sprint** rule
> — [[one-pr-per-sprint]] — landing as its own `dev`→`main` PR.)

## Diagnosis (from the kept trace) — grounded, not guessed
Re-ran L0 (`list the files, then call task_complete`) on `llama3.2:1b`, `--keep-workspace`:
- **`terminator: repetition_guard`**, `tools_called: ['list_dir', 'list_dir']`, `repetition_guard_fires: 1`, `expectations_ok: true`, `tools_ok: false`.
- Trace: turn 1 `list_dir` → turn 2 `list_dir` (repeat #1 → **Warn** + nudge) → turn 3 `list_dir` (repeat #2 → **Stop**) → `session_end`.

**The failure is repetition-not-termination.** The 1B emits a *correct* `list_dir`,
gets the result, and — instead of recognizing the task is done and calling
`task_complete` — **calls `list_dir` again**. It can produce a valid tool call but
can't *transition to completion*. The guard catches the loop (good safety) but the
task fails.

## What's already there (and why it's not enough)
`crates/ferric-loop/src/run.rs:258` already nudges on the first repeat: *"You are
repeating the same tool calls. Take a different action, or call task_complete if the
task is done."* The 1B **ignored it** and repeated a third time. The nudge is **soft
and indirect** ("take a different action, *or* … *if* done") — a 1B needs a direct
imperative, not a conditional.

## Decisions Reviewed
- **ADR-013** — `task_complete` is the structured terminator; the loop already offers it every turn.
- **Repetition guard (sprint 1)** — two-strike: Warn (nudge) → Stop. The nudge is the only steering lever before Stop; sharpening it is the minimal, in-place mitigation.
- **ADR-030 (s21)** — the fleet finding this sprint follows up: single-tool-call reliability ≠ agentic capability. A new ADR records the *mechanism* (repeat-not-terminate) + whether the nudge helps.

## Mitigation (settled, bounded)
Sharpen the first-repeat nudge into a **direct imperative that names the repeated
tool**: *"You already called `<tool>` and have the result — do not call it again. If
the task is finished, call task_complete now with a one-sentence summary."* Rationale:
small models follow "do X now" far better than "do something different, or X if done."
Then **re-bench the 1B** (L0–L6) to measure whether it now completes any level.

## Risk
- **It may not help** — the 1B's limit could be deeper than wording. That's a valid outcome: ship the sharper nudge (it can't hurt larger models — they already terminate) and document the 1B's agentic ceiling honestly (ADR). Either way the sprint has a concrete deliverable (nudge change + a measured result).
- **Regression** — a repetition test may assert the old nudge text; update it. The guard's two-strike *behavior* is unchanged (wording only).

## Recommended approach
T-2201: sharpen the repetition nudge in `run.rs` (name the repeated tool, imperative
task_complete) + update the repetition test(s). T-2202: re-bench `llama3.2:1b` L0–L6
→ does it complete any level now? + ADR + docs. AI-verifiable: the loop unit test for
the new nudge; the live 1B re-bench is the measurement.
