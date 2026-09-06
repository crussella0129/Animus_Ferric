# Sprint 121 Test report — accepted with caveats

Corrected qualified source: `a417c5d00361fd25a238346e5015fb07ed5ae7c7`.
The independent final [Test critique](critique.md) returned
`proceed-with-caveats` after rereading the corrected source, all required
intent/plan/result artifacts and the final grader-confirmation adjustment.
This report was written only after that verdict. The [initial blocking critique](critique-initial.md)
remains retained.

## Accepted increment

The [sixteen-clause coverage map](coverage-ledger.md), [unit record](unit-tests.md),
[composed integration record](integration-tests.md) and [fresh E2E record](e2e-tests.md)
prove the locked Test-stage promises:

- E01: positive context-reserve-bounded explicit action cap, unchanged omitted
  defaults and authority, actual sampler/HTTP/trace agreement, invocation-scoped
  shell-correct resume, exact large scripted writes and no truncated dispatch.
- E02: checked positive finite benchmark scales, exact fractional durations,
  compatible real/mock/continuation argv, separate parent/child termination
  attribution, no-clobber digest-bound sidecars and checked process cleanup.
- E03: modified-budget evidence cannot publish durable calibration, even through
  direct library calls or forged derived flags; actual shared publication
  paths preserve absent/valid/unrelated/malformed profiles and truthful output.
- E04-A/B: the compact `cargo r` front door and expert compatibility remain
  unchanged; deterministic source-owned fault tests and a fresh existing-model
  smoke prove the narrow explicit-budget path with checked cleanup.
- E04-C Test-stage native/compile gates passed at the exact corrected head.
  Loop, the extra fresh post-Loop audit and the final PR checkpoint are separate
  mandatory later obligations before offering the sprint for merge.

INT-0007 advances only the explicit-control/attribution portions of AC-11/12;
INT-0008 AC-6/11/12 are preserved. Neither complete intent is realized.

## Qualification results

| Gate | Actual result |
|---|---|
| Root Windows canonical workspace | 1,303 passed, zero failed, thirteen documented ignores; all 79 suite confirmations retained. |
| Exact-head Windows CI workspace | Same 1,303/0/13. |
| Exact-head isolated Linux CI workspace | 1,309/0/9. |
| Backend-free CLI, Windows/Linux | 416/0/0 each. |
| Native lifecycle, Windows/Linux | Five/six passed, zero failures/ignores. |
| Formatting and Clippy | Workspace/included fixture format and all required warnings-denied default/backend-free/lifecycle/backend Clippy gates passed. |
| ARM64 | Both default workspace and lifecycle all-target cross-compile checks passed; not native runtime evidence. |
| Additional existing Ubuntu WSL | 35 process-free core tests plus core Clippy and native formatting passed; full namespace runtime gate unavailable without sudo authorization. |
| Corrected-source live smoke | One passed in 15.07 s; actual cap/trace/identity agreement, joined watchdogs and checked owned-engine/parent cleanup. |

All eight jobs of [canonical CI 34004554100](ci-checkpoint-003.md) passed at the
qualified source. Source, manifests, lockfile, CI and tools have not changed
since that qualification. Subsequent Book-only commits must be verified as
such, and the sole PR's final head/checks remain mandatory.

The live trial used the existing repository Qwen2.5-Coder-7B-Instruct Q4_K_M
and installed llama-server `10034 (505b1ed15)`, CPU-only/context 4096/explicit
cap 1024. It returned non-truncated `task_complete` with summary `Ferric budget
smoke complete`; the raw response, original trace, independently verified
digests, settings and stage timings are in `live-test-002`. Setup/request/
parent/cleanup/whole-test limits remained 90/30/150/5/180 seconds. No downloads,
borrowed-server termination, manual process repair or changed model settings
were used to obtain acceptance. Provider-admission observation in this live
fixture is distinct from the actual HTTP wire tests in integration evidence.

## Retained failures and caveat disposition

Original Windows CI `34002834811` failed the existing first-run journey; its
native cause was not recorded and **remains unknown**. The 32-journey local
diagnostic sample and instrumented green checkpoint were non-reproductions,
not repairs. Independent review found a concrete separate accept-reset fixture
defect. Its narrowly classified correction has four new regressions and an
executed mutation control that fails under the old decision after checked
cleanup. Corrected source then passed unchanged canonical gates and fresh live
qualification. This accepts that source, not a retrospective root-cause claim.

C-003 is accepted with the reviewer's stated rationale: preserve the original
failure, intermediate diagnostics and negative control; retain T-12026/27 for
native admission/parallel robustness; treat any future canonical recurrence as
a blocker. There is no diagnosed production ownership repair or arbitrary
parallel-load qualification claim. Earlier Build disk/compile/HTTP/hash/live
failures remain in Build verification and append-only raw records.

The release live pass does not establish debug-profile live timing, larger
action/model coding success, hardware/speed calibration, reasoning/compaction
tuning, the frozen application trial or Sprint Loops support. The larger-model
T-11506 → T-11410 → T-11412 sequence remains open. Linux namespace evidence does
not resolve ordinary-host visibility or abrupt nested-group ownership;
T-11707/T-11904 and macOS/platform limits remain explicit. The opt-in live smoke
is Windows-qualified; WSL does not expand that boundary.

## Required handoff boundary

Link this accepted Test report to the active intents, reconcile the delivered
T-11505 umbrella and remaining work, adjust confidence once, validate/close
Loop, and obtain the owner's extra fresh independent phase-completeness/code
audit. Only afterward commit/push/confirm, open exactly one `dev` → `main` PR,
verify final head/checks, restore/hash-check the protected unrelated Sprint 114
edit, and stop for the owner to merge. This Test report does not certify those
future actions as already completed.
