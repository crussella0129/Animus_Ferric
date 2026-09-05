# Sprint 120 Test report

**Historical accepted-source report; final checkpoint acceptance is reopened.**
Two original CI runs at Book-only checkpoint `3b966dd` repeated the bounded
PowerShell quoting fixture timeout. See [checkpoint diagnosis](checkpoint-diagnosis.md)
and the current blocking critique. The report below preserves what passed at
`0ec5a0e`; it does not waive the later Windows gate or claim a diagnostic patch
resolved the failure.

**Pass for the locked prepared-host increment**, after the independent
[Test critique](critique.md) returned `clean`. Tested implementation:
`0ec5a0eb0f465e8220b7f2010428aed3d6f2975d`.
[CI run 33947290181](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181)
passed all eight required jobs at that exact head. Later Book-only commits do
not change the tested implementation; the PR checkpoint must verify that fact.

## Accepted promises

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

## Verification results

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

## Limits and disposition

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
