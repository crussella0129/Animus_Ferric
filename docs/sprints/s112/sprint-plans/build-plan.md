Finalized - DO NOT EDIT

# Sprint 112 Build Plan

1. Freeze commit `1b0c0dfef52400d2686b9e69c1e6e623da71bfda` as the
   pre-change reference and capture the current live-server smoke result.
2. Repair benchmark process isolation, budget enforcement, exact completion,
   executable grading, repetition semantics, provenance, and fail-closed result
   loading before collecting a new score.
3. Introduce additive trace events for committed turns, recovery checkpoints,
   and paused sessions; preserve parsing of old JSONL traces.
4. Make replay validate canonical workspace identity and reconstruct only
   explicitly committed state. Make recoverable stops resumable more than once.
5. Add structured user-input requests and answer-aware continuation.
6. Add named, fixed-argv checks and require fresh passing evidence after the
   latest mutation when a check policy is configured.
7. Build the 24-task internal matrix, compare policy variants, and retain enough
   metadata/traces to reproduce every row.
8. Run focused tests, full workspace quality gates, real-server lifecycle,
   repeated live acceptance, and document honest confidence bounds.

