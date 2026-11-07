# Sprint 130 Meta

- **Sprint number:** 130
- **Book schema version:** 2
- **Start timestamp:** 2026-11-07T08:30:00Z
- **End timestamp:** 2026-11-07T11:30:00Z
- **Model:** claude-opus-5-5
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Retire the superseded surfaces the owner approved
  (INT-0018, T-12906).
  - **Retired:**
    - Tailscale Serve, plus schema-1 registrations with `server adopt`;
    - the paired autonomy A/B runner;
    - EvidencePlanner;
    - replay of traces from before sprint 113;
    - the backend-free build;
    - the tailnet research retriever;
    - the tools installers.
  - **Size:** 131,421 → 112,457 lines of Rust; −19.7% since the start of
    sprint 129.
  - **R-4** (the Legacy `Tool::run` path) proved not to be a pure removal
    and returns to the owner as T-13006.
  - **T-12907:** the analysis is in the build plan; awaiting the owner.
- **Intents:** [INT-0018](../../intents/INT-0018-retire-superseded-surfaces.md), [INT-0017](../../intents/INT-0017-minimal-code.md)
- **Completion evidence:**
  - **Done:** INT-0018 AC-1 to AC-3 and AC-5 to AC-9 (T-13001 to T-13005).
  - **Gates:** CI green on every commit. In this sandbox the only failures
    are the 19 known lifecycle tests.
  - **Open:** T-13006 and T-12907. See the
    [test report](sprint-tests/test-report.md).
- **Checkpoint:** (the sprint PR)
