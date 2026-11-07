# Sprint 129 Meta

- **Sprint number:** 129
- **Book schema version:** 2
- **Start timestamp:** 2026-11-07T03:00:00Z
- **End timestamp:** 2026-11-07T07:55:00Z
- **Model:** claude-opus-5-5
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Minimal code (INT-0017, owner direction 2026-11-07).
  - **Analysis:** eight read-only analysts swept the whole workspace for
    redundancy (117 findings with evidence), and one plan separated
    behaviour-preserving consolidations, defects and owner-gated
    retirements.
  - **Consolidation:** each area landed as its own verified commit (loop,
    tools, bench, CLI, model I/O, server lifecycle, engine and process).
  - **Crate folds:** six single-consumer crates folded into their users.
  - **Result:** 140,082 → 131,421 lines of Rust (−6.2%), 20 → 14 crates,
    and 11 defects fixed (D-1 to D-11), each with a regression test,
    guard rule or deletion.
  - **Also:** the open Dependabot updates were applied on `dev`.
- **Intents:** [INT-0017](../../intents/INT-0017-minimal-code.md), [INT-0010](../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md), [INT-0016](../../intents/INT-0016-install-run-done.md) (D-3, D-7 and D-11 touch the first-run path)
- **Completion evidence:**
  - **Done:** INT-0017 AC-1 to AC-5 (T-12901 to T-12905).
  - **Gates:** fmt and clippy are clean in every build, including Windows
    and the lifecycle fixture. GitHub CI is green on Ubuntu and Windows
    from 31c57b5. In this sandbox the only failures are the 19 known
    lifecycle tests.
  - **Open, for the owner:**
    - the retirements (T-12906);
    - the Linux non-root engine-ownership check (T-12907, high priority);
    - deferred consolidations (T-12908).
  - **CI incident:** CI on Linux was red from 06a33be until 31c57b5,
    because of a zombie adopted by the test process (D-11). Commit 2a77cac
    does not build on its own; 31c57b5 completes it. See the
    [test report](sprint-tests/test-report.md).
- **Checkpoint:** https://github.com/crussella0129/Animus_Ferric/pull/126
