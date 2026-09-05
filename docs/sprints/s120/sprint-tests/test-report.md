# Sprint 120 Test report

**Pass for the locked increment at `4f4e4f04d4ee132f9df9bb422be88a5ce366915d`,
with the controlled-test-schedule caveat below.** The renewed independent
[Test critique](critique.md) returned `proceed-with-caveats` before this report
was renewed. The earlier accepted-source report is retained separately below.

## Current qualification and caveat

Both exact-head [push run 33949875039](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039)
and [PR run 33949876363](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363)
passed all eight jobs without reruns. All 75 workspace suite confirmations per
platform/run were checked: Windows 1,247/0 failed/seven intentional ignores,
Linux 1,253/0/five. Backend-free CLI passed 407/0/0 per platform; native lifecycle
passed five Windows and six Linux. Formatting, warnings-denied clippy and
backend-enabled ARM64 compile gates passed. The [integration map](integration-tests.md)
binds every locked clause to its named assertions and current execution route.

Fresh local canonical workspace, backend-free, lifecycle and lint gates also
passed. The separately executed model test passed in 7.02s with the expected
answer, checked cleanup and lock reacquisition. A fresh actual Cargo terminal
session answered and exited zero through owned cleanup; both are bound to this
source in [E2E evidence](e2e-tests.md). No source changed during qualification.

**C-002 is mitigated by controlled scheduling, not a proven historical fix.**
Both native workspace gates isolate unrelated test bodies, retaining every
test, original deadline, argument assertion and checked cleanup. Explicit
startup races still create simultaneous worker threads and bounded barriers.
The earlier parallel Windows timeouts and their unknown stage remain in
[checkpoint diagnosis](checkpoint-diagnosis.md); T-12027 retains parallel-suite
robustness work. T-12026 is separate Windows admission hardening. Any recurrence
under the qualified schedule is a blocker, not grounds for repeated retries.

INT-0005/6/8 remain active and partial. Prepared resources are required; no
acquisition, calibration, complete resume, full Work cancellation, platform
parity or model-built application/skill qualification is claimed. Renewed Loop
closure and another separate post-Loop audit must follow this Test pass within
the existing PR 108; the owner alone merges.

## Historical accepted-source report at 0ec5a0e

**Pass for the locked prepared-host increment**, after the independent
[historical Test critique](https://github.com/crussella0129/Animus_Ferric/blob/dc9c900253683875b179fccec0649c0bb116c5e1/docs/sprints/s120/sprint-tests/critique.md)
returned `clean`. Tested implementation:
`0ec5a0eb0f465e8220b7f2010428aed3d6f2975d`.
[CI run 33947290181](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181)
passed all eight required jobs at that exact head. Later Book-only commits do
not change the tested implementation; the PR checkpoint must verify that fact.

### Accepted promises

The [clause-level integration map](integration-tests.md) binds every locked
E01–E06 clause to named, executed assertions and the affected intent criteria.
The [unit record](unit-tests.md), [canonical CI confirmations](ci-results.md)
and [fresh live/terminal evidence](e2e-tests.md) retain actual commands/results.

- E01 / INT-0005: existing Python admission survives RustPython 0.5 with
  invalid/unchecked distinctions, atomic publication and Legacy compatibility.
- E02 / INT-0006: invalid present configuration fails closed, selected workspace
  drives discovery, effective streaming is honored, and omitted resume policy
  inherits safely. This is the AC-5/6 configuration increment only.
- E03 / INT-0008: bounded prepared-host startup, persistent workspace admission,
  model preference, verified owned/borrowed lifetimes and scoped cleanup.
- E04 / INT-0008: useful ordinary launch, at most three setup decisions, structural
  Ask isolation, fresh folder consent for Work, four primary actions, expert
  compatibility and concise actionable errors. C-001's actual failure paths are
  corrected and executed, not covered only by synthetic copy.
- E05 / INT-0008: cancellation across response phases and byte-correct UTF-8 SSE,
  with finite joined fixtures and prepared-endpoint credential boundaries.
- E06: composed negative/positive journeys, source execution, current short docs,
  required native/compile gates and separate real-model/terminal acceptance.

### Verification results

| Gate | Result |
|---|---|
| Windows workspace, local and CI | 1,247 passed / 0 failed / 7 intentional ignores |
| Isolated native Linux workspace CI | 1,253 passed / 0 failed / 5 intentional ignores |
| Backend-free CLI, Windows/Linux | 407 passed / 0 failed / 0 ignored each |
| Native lifecycle, Windows/Linux | 5 / 6 passed respectively |
| Workspace/default/backend-free/lifecycle clippy; formatting | Passed, warnings denied |
| ARM64 workspace and lifecycle | Both backend-enabled compile checks passed; not native runtime evidence |
| Exact local H/HU/S/P/PY/M/CLI | 14; 17 (+ separate live ignore); 38; 47; 16; 15; 72 passed |
| Real-model L and actual Cargo PTY | Passed independently; expected answer and checked owned cleanup |

The live trial used the existing Qwen2.5-Coder-7B-Instruct Q4_K_M file and
llama-server `10034 (505b1ed15)`, CPU-only/context 4096/temperature 0. Ready took
3.999448 seconds, first response 4.6735217 seconds; the source-owned live test
finished in 5.96 seconds. Actual answer: `Ferric is ready.` Its trace ended
`answered`; cleanup and subsequent workspace admission passed. The separate
terminal run also exited zero after `/quit`. No manual process termination
converted a failed run into success.

### Limits and disposition

Earlier Build failures, two failed CI candidates and the initially green-but-
insufficient E04-D candidate remain recorded. Their corrections changed no
locked acceptance criterion or production timeout/authority boundary.

INT-0005, INT-0006 and INT-0008 remain **active**, not realized. Python maintenance
does not deliver Rust/JavaScript parsing; configuration admission does not retire
inert policy fields or change API reload lifetime. The human front door requires
prepared resources, does not download/calibrate, uses unmeasured conservative
defaults, and does not provide full workflow resume or application qualification.
Linux positive ownership is proved in the stated isolated environment, not on
arbitrary shared hosts; macOS automatic startup remains unsupported. Git snapshot
work can still delay Work cancellation (T-12024). INT-0007 receives no new
hardware, Qwen3.8, medium-horizon or autonomous Sprint Loops acceptance claim.

Loop reconciliation, Book validation/close and the extra post-Loop audit follow
this Test pass. They must precede the sole dev-to-main PR; the owner alone merges.
