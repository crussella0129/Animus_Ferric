# Sprint 22 Meta

- **Sprint number:** 22
- **Start timestamp:** 2026-06-26T03:54:35Z
- **End timestamp:** 2026-06-26T04:40:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Diagnosed why llama3.2:1b fails L0 (from the kept trace): repeat-not-terminate (re-calls list_dir instead of task_complete) + semantic flailing (L2: 15 make_dirs → max_turns, which the repetition guard misses). Sharpened the first-repeat nudge into a direct imperative naming the repeated tool — but the re-bench showed **no change** (still measured_level none, identical modes), disproving the wording hypothesis. The 1B's multi-turn failure is a genuine capability limit, not prompt text (ADR-031). The nudge ships anyway (better wording, helps mid-tier models, can't regress capable ones). First sprint under the one-PR-per-sprint rule.
