# Sprint 27 Research Report — A no-progress guard for "semantic flailing" (ADR-031)

## Sprint goal (in my words)
ADR-031 named two multi-turn failure modes from the 1B's kept trace. Sprint 22 hardened
the harness against the first (**repeat-not-terminate** — identical calls — via the
repetition guard's sharper nudge). The **second is still unguarded: "semantic
flailing"** — the model calls *the same tool with different args* over and over
(`make_dir` ×15 with different paths) and never completes, so it grinds to `max_turns`.
The repetition guard misses this by design (it matches identical action *signatures*,
so same-tool/different-args isn't a "repeat"). This sprint adds the complementary
**no-progress guard**: detect a same-tool-name streak and stop early with a precise
`StopReason::NoProgress`, instead of burning every turn to `max_turns`.

**Honest scope (per ADR-031):** this will *not* make a weak model succeed — the 1B's
ceiling is a capability limit, and nudging didn't move it. The value is (1) **bounding
wasted compute** on a stuck model (fail fast — stop at ~6 turns, not 15/40/80), and (2)
a **precise diagnostic** in the trace + bench (`no_progress` vs the ambiguous
`max_turns`), which lets the leaderboard distinguish *flailing* from *ran-out-of-turns-
mid-productive-work*.

## Decisions Reviewed
- **ADR-031 (sprint 22)** — the source of this work. It explicitly identifies "semantic
  flailing" as the unguarded mode (`make_dir` ×15, different paths → `max_turns`; "the
  repetition guard misses this — it matches identical action *signatures*"). This sprint
  implements the guard ADR-031 implies. No prior decision is revised.
- **ADR-013** — `task_complete` is the structured terminator; the guard nudges toward it.
- **ADR-019/030** — `measured_level` + the bench ladder; the new stop reason classifies
  as a non-completion (correct), giving the bench a sharper failure signal.

## Existing Code Survey
| File | Role / relevance |
|---|---|
| `crates/ferric-loop/src/repetition.rs` | The guard to mirror: `RepetitionGuard{last_signature, consecutive_repeats}`, `observe(&[ToolCall])->Verdict{Proceed,Warn,Stop}`, 2-strike. `signature_of` hashes name **+ args**. The new guard is the same shape but **names-only**. |
| `crates/ferric-loop/src/run.rs` | The loop. Guard wired at ~L252 (`repetition.observe(&actions)`) → Warn nudges, Stop breaks `RepetitionGuard`. The new guard slots in right after, same pattern. |
| `crates/ferric-loop/src/outcome.rs` | `StopReason` enum + `as_str()`. Add `NoProgress => "no_progress"`. |
| `crates/ferric-core/src/scale.rs` | `tier_row`: `max_turns` = Nano 15 / Small 20 / Medium 25 / Large 40 / Xl 60 / Ultra 80. Calibrates the streak threshold — a stop at ~6 turns is well under every tier. |
| `crates/ferric-trace/src/event.rs` | `Event::RepetitionGuard{action}` (typed guard event). Add a parallel `NoProgressGuard{action}` (additive serde variant; unknown tags already fall to `ParsedEvent::Unknown`). |
| `crates/ferric-bench/src/verify.rs` | `completed()` passes only on `None|task_complete|final_text` terminators → a `no_progress` terminator classifies as a **non-completion automatically. No change needed.** `parse_trace` matches specific events with a catch-all. |
| `crates/ferric-loop/tests/repetition_tests.rs` | The integration-test harness to mirror (`run_scripted`, `tool_completion`, `nano_policy`, `session_end_reason`, `guard_actions`). |
| `crates/ferric-loop/src/lib.rs` | Module exports (`pub mod repetition` etc.) — add `progress`. |

## External Sources
None — this is internal harness design grounded in the ADR-031 kept-trace evidence; no
new library or vendor behavior is involved.

## Risks / unknowns / dependencies
- **False positives on legitimately repetitive tasks** (e.g. "write 6 files" = `write_file`
  ×6). The harness can't semantically tell "productive repetition" from "flailing." Mitigated
  by: (a) a **conservative threshold** comfortably above realistic same-tool runs at a tier
  yet well below `max_turns`; (b) a **Warn/nudge one turn before the Stop** (course-correction
  for a capable model momentarily stuck); (c) **names-set granularity** (a turn mixing tools
  resets the streak); (d) `max_turns` remains the ultimate backstop. The tradeoff is documented
  in the ADR.
- **Guard composition:** repetition (identical-sig, 2-strike) fires *first* and earlier; the
  progress guard only catches the different-args case the repetition guard lets through. They
  don't conflict (different signatures, different thresholds).
- **Additive trace variant:** adding `Event::NoProgressGuard` is backward-compatible for writing;
  readers in-repo rebuild together, and `ParsedEvent::Unknown` covers any stragglers.

## Recommended approach
A **`ProgressGuard`** (new `crates/ferric-loop/src/progress.rs`) mirroring `RepetitionGuard`:
- signature = **sorted-unique tool names** of the turn's calls (arg-insensitive);
- track a consecutive same-names `streak`; reset on change;
- `observe(&[ToolCall]) -> Verdict`: **Warn** at `WARN_AT` consecutive matches, **Stop** at
  `STOP_AT` (e.g. 4 / 5 → the tool is used ~6 turns before the stop — under Nano's 15, far
  under Large's 40).
- Wire after the repetition guard in `run.rs`: Warn → emit `Event::NoProgressGuard{"warned"}`
  + a nudge ("You've called <tool> repeatedly without finishing — if the task is complete call
  task_complete, otherwise use a different tool/approach"); Stop → emit `{"stopped"}` + `break
  StopReason::NoProgress`.
- Add `StopReason::NoProgress` ("no_progress") and `Event::NoProgressGuard{action}`.
- Tests: a unit test on `ProgressGuard` (streak warn→stop; resets on a tool-name change; a
  single repeated tool with **different args** trips it where the repetition guard wouldn't),
  and an integration test mirroring `repetition_tests.rs` (scripted same-tool/different-args
  turns → `StopReason::NoProgress`, trace `["warned","stopped"]`, session-end `no_progress`).

### Alternative considered — workspace-state progress metric (rejected)
Hash the workspace each turn; stop if it doesn't change for K turns. **Rejected:** the
canonical flail (`make_dir` with a *new* path each time) *does* mutate the workspace, so a
state-change metric sees false "progress" and never trips — it fails on the exact case we're
targeting. It's also costlier (hash the tree each turn). A **repeated-failure guard** (last K
results all errors) is a *different* mode (the flail *succeeds* each call) — noted as future work.
