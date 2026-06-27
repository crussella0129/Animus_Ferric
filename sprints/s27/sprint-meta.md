# Sprint 27 Meta

- **Sprint number:** 27
- **Start timestamp:** 2026-06-27T14:40:25Z
- **End timestamp:** 2026-06-27T15:10:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Closed ADR-031's second, still-unguarded multi-turn failure mode — "semantic flailing" (the model calls the same tool with *different* args every turn and never completes, grinding to `max_turns`). The repetition guard misses it by design (it hashes name **+ args**). Added `ProgressGuard` (`crates/ferric-loop/src/progress.rs`) mirroring `RepetitionGuard` but canonicalizing only the sorted-unique tool **names** (arg-insensitive): Warn at `WARN_AT=4`, Stop at `STOP_AT=5` → `StopReason::NoProgress` (`Event::NoProgressGuard{action}`), wired after the repetition guard in `run.rs`. Threshold ~6 turns — above realistic same-tool runs, under every tier's `max_turns`; the guards compose. 6 new tests incl. the defining contrast (`ProgressGuard`→Stop where `RepetitionGuard`→Proceed on the same input). No bench change. ADR-037; README Status 27. Honest scope: bounds wasted compute + sharpens the diagnostic, does not lift a capability ceiling. One PR per sprint; `dev` clean (PR #12 merged).
