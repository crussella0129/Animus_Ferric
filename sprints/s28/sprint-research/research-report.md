# Sprint 28 Research Report — A repeated-failure guard (the third loop-hardening guard)

## Sprint goal (in my words)
Ferric's loop now has two stuck-loop guards: the **repetition guard** (identical action
signature, name+args) and the **no-progress guard** (same tool *name* streak, ADR-037).
Both key off the *actions* a model emits. Neither keys off whether those actions **work**.
A model can emit a *different* tool every turn that *all error* — wrong paths, denied
permissions, malformed args — and never recover: the repetition guard resets (different
signature), the no-progress guard resets (different name), so it grinds to `max_turns`.
This sprint adds the **repeated-failure guard**: track consecutive turns whose dispatched
tools **all errored**, and stop early with a precise `StopReason::RepeatedFailure`. It
completes the guard family (repetition / no-progress / repeated-failure) and is the only
one that keys off tool *results*. This exact mode was flagged as future work in the
ADR-037 research ("a repeated-failure guard … is a *different* mode — the flail succeeds
each call").

**Honest scope (consistent with ADR-031/037):** this does not make a weak model succeed —
it bounds wasted compute on a model stuck failing, and emits a precise `repeated_failure`
diagnostic distinct from `max_turns`.

## Decisions Reviewed
- **ADR-037 (sprint 27)** — the no-progress guard + the explicit note that a repeated-
  failure guard is a distinct, unbuilt mode. This sprint builds it; no revision.
- **ADR-031 (sprint 22)** — the failure-mode taxonomy this guard family addresses.
- **ADR-013** — `task_complete` terminator (the guard nudges toward it / a different approach).
- **ADR-019/030** — bench ladder; `repeated_failure` classifies as a non-completion (correct).

## Existing Code Survey
| File | Role / relevance |
|---|---|
| `crates/ferric-loop/src/run.rs` | The dispatch loop (~L304-334) produces `is_error` per call from `dispatch()`. The new guard observes the turn's *results* **after** the dispatch loop (unlike no-progress/repetition, which observe actions before). Gate on `terminate_with.is_none()` (a terminating turn is success). |
| `crates/ferric-loop/src/progress.rs` | The sibling to mirror in shape (a small struct + `Verdict` + a streak + WARN/STOP consts). |
| `crates/ferric-loop/src/repetition.rs` | `Verdict{Proceed,Warn,Stop}` — reused (as the no-progress guard does). |
| `crates/ferric-loop/src/outcome.rs` | `StopReason` + `as_str()`. Add `RepeatedFailure => "repeated_failure"`. |
| `crates/ferric-trace/src/event.rs` | Typed guard events (`RepetitionGuard`, `NoProgressGuard`). Add `FailureGuard{action}`. |
| `crates/ferric-loop/tests/common/mod.rs` | `kinds()` is an **exhaustive** `Event` match — needs a `FailureGuard` arm (compiler-enforced). |
| `crates/ferric-cli/src/trace_cmd.rs` | Also an exhaustive `Event` render match — needs a `FailureGuard` arm. |
| `crates/ferric-bench/src/verify.rs` | `completed()` passes only on `None|task_complete|final_text` → `repeated_failure` is a non-completion automatically. **No bench change.** |
| `crates/ferric-tools/src/registry.rs` (`ExecuteOutcome`) | `dispatch()` sets `is_error` for `Denied`, `UnknownTool`, and `Completed{is_error}` — so a denial or a tool error both count as a failed call. |

## External Sources
None — internal harness design grounded in ADR-031/037.

## Risks / unknowns / dependencies
- **False positives:** a model legitimately probing (e.g. a `read_file` that 404s while
  exploring) could string a couple of errors. Mitigated by a small threshold *with a warn
  first* (WARN at 2 consecutive all-error turns, STOP at 3) and by the "all dispatched
  calls errored" definition — any *successful* call in a turn resets the streak (mixed
  turns = partial progress). `max_turns` remains the backstop.
- **Failure-turn definition:** a turn counts as failed iff it dispatched ≥1 (non-terminator)
  tool and **every** dispatched tool errored. A turn with no tool calls (terminator only, or
  a no-action nudge turn) does not count and does not reset (it's not a result-bearing turn) —
  simplest is to only `observe` when ≥1 tool dispatched.
- **Guard composition:** runs after dispatch, independent of repetition/no-progress (which
  run before dispatch). Catches the "different tools, all failing" mode the other two reset on;
  for same-failing-call cases the repetition guard still fires first (earlier threshold).
- **Additive trace variant:** `FailureGuard` is additive (serde-tagged enum; unknown tags →
  `ParsedEvent::Unknown`); two exhaustive in-repo matches get a new arm (compiler-checked).

## Recommended approach
A **`FailureGuard`** (`crates/ferric-loop/src/failure.rs`) mirroring `ProgressGuard`:
- `observe_turn(dispatched: usize, errored: usize) -> Verdict`: `all_failed = dispatched > 0
  && errored == dispatched`; reset on `!all_failed`; **Warn** at `WARN_AT=2`, **Stop** at
  `STOP_AT=3` consecutive all-failed turns.
- Wire in `run.rs`: during the dispatch loop, count dispatched non-terminator calls + how
  many returned `is_error`; after the loop, if `terminate_with.is_none()`, `failure.observe_turn(..)`
  → `Warn` emits `Event::FailureGuard{"warned"}` + a nudge ("Your last tool call(s) failed —
  read the error and try a different approach, or call task_complete if you cannot proceed");
  `Stop` emits `{"stopped"}` + `break 'outer StopReason::RepeatedFailure`.
- Add `StopReason::RepeatedFailure` (`"repeated_failure"`) + `Event::FailureGuard{action}`.
- Tests: unit on `FailureGuard` (warn→stop on all-failed streak; a successful call resets;
  a no-tool turn doesn't trip it); integration via `run_scripted` with a tool that errors
  (e.g. `read_file` on a missing path, or a denied path) for 3 turns → `RepeatedFailure`,
  trace `["warned","stopped"]`, `session_end == "repeated_failure"`.

### Alternative considered — fold failure-tracking into the no-progress guard (rejected)
Overloading one guard with both name-streak and error-streak logic muddies two orthogonal
signals (one keys off actions pre-dispatch, the other off results post-dispatch) and would
force the no-progress guard to move after dispatch. Three small, single-purpose guards are
clearer and independently testable — consistent with the existing `repetition`/`progress`
split.
