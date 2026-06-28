# Plan Critique — Sprint 31

> Self-critique against `prompts/plan-critic.md` (no subagent spawn — autonomous-ish, user-steered).

## Concerns

### C-001: The `Retriever` trait is the keystone — wrong shape is expensive
- **Failure mode:** premature-abstraction / churn-later
- **Response:** surfaced explicitly at the ExitPlanMode checkpoint (user-approved). The shape is
  minimal (`plane`/`available`/`retrieve`) and driven by the *known* later planes: `available()`
  exists because web/tailnet can be offline; `async` exists because they're network I/O. Not
  speculative — each method earns its place from inc 3/4 requirements.

### C-002: Is a `LocalFsRetriever` redundant with the `search_files` tool?
- **Failure mode:** duplicate-capability
- **Response (reject):** different role. `search_files` is a **model-callable tool** returning
  match-lines into the agent loop. `LocalFsRetriever` is a **programmatic research source** that
  returns whole candidate **documents** to the **quarantine** (never to the planner). It reuses
  the walk *pattern* but serves the Ornstein pipeline, not the tool registry.

### C-003: Reading local files as "untrusted" — overkill?
- **Failure mode:** misplaced-trust
- **Response:** correct and deliberate. A local file can be a downloaded doc, a cloned repo's
  README, a NAS share — all carry injection risk. Routing *every* source (even local) through the
  quarantine is the invariant that makes the multi-source design safe and uniform. The retriever
  also confines to `root` and doesn't follow symlinks (escape-safety) — defense in depth.

### C-004: Symlink test on Windows
- **Failure mode:** flaky-test
- **Response:** symlink *creation* needs privilege on Windows, so the skip-symlinks behavior is
  implemented + documented but not asserted by a Windows-flaky test. The noise-dir + binary skips
  are tested (portable).

### C-005: `research()` swallowing unavailability as empty
- **Failure mode:** silent-no-op
- **Response:** intentional + tested — a capability-probed multi-source system runs the planes
  that are live and skips those that aren't; an offline tailnet shouldn't fail the whole research.
  The orchestrator (inc 5) will report which planes ran. For now it's a documented `Ok(vec![])`.

## Confidence
`clean` — a small, additive increment in an existing crate: the keystone trait (user-reviewed) +
one safe source plane + the end-to-end pipeline, all deterministically tested (temp dir +
MockProvider). It executes directly against the ADR-040 roadmap and the user's chosen build order.
