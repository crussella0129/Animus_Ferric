# Sprint 120 final checkpoint diagnosis

## Repeated blocker at unchanged source

PR 108 was opened after actual closure and the clean independent extra audit.
Its metadata checkpoint `3b966dd583ddf648c7a505a9a10064a999ff0a6f` was pushed
and independently confirmed; the PR has base `main`, head `dev` and 25
Sprint 120 commits at this point. Every change after accepted source `0ec5a0e`
was Book-only, and locked plans were unchanged.

Nevertheless, the final Windows workspace gate failed in two original runs:

| Run/job | Observed result |
|---|---|
| [Push attempt 1](https://github.com/crussella0129/Animus_Ferric/actions/runs/33948474675/job/101258709869) | Seven jobs passed; Windows CLI units 380 passed, one failed, one live ignore, 15.43s |
| [Original PR run](https://github.com/crussella0129/Animus_Ferric/actions/runs/33948476272/job/101258714622) | Seven jobs passed; Windows CLI units 380 passed, one failed, one live ignore, 23.16s |

Both fail `query::tests::powershell_quote_round_trips_argv`, with
`source test child exceeded 10s after checked cleanup`, before exit-status or
argument equality assertions. This is not an observed argument mismatch and
does not show failed cleanup. The ten-second clock includes native suspended
process creation, Job admission and thread resume, not just PowerShell script
execution. The adapter discards timeout wall/output details, so the original
logs cannot distinguish those stages. Runner contention/cold startup remains
a hypothesis, not an established cause.

After the first failure, independent read-only review supported **one**
unchanged-head failed-job rerun. It was requested before the second original
run's failure became known; its outcome must remain separate and cannot erase
the recurrence. No further retry, higher deadline, skipped assertion or manual
process cleanup is authorized as a substitute for diagnosis.

Actual authorized rerun outcome: push attempt 2 succeeded. Its new
[Windows workspace job](https://github.com/crussella0129/Animus_Ferric/actions/runs/33948474675/job/101259576960)
explicitly passed the quoting test; all 75 suite summaries confirm 1,247 passed,
zero failed, seven intentional ignores. CLI units passed 381/0/1 in 12.09s.
The original PR run still failed, leaving 15 successful checks and one failed
at this checkpoint. No further unchanged-head retry was requested; success
of the one rerun does not resolve C-002.

## Bounded local and adjacent evidence

At unchanged checkpoint `3b966dd`, root executed through source-aware Cargo:

- `cargo test -p ferric-cli --bin ferric --locked query::tests::powershell_quote_round_trips_argv -- --exact --nocapture --test-threads=1`:
  one passed in 0.26s, checked source cleanup, exit zero.
- `cargo test -p ferric-cli --bin ferric --locked --quiet`: 381 passed, zero
  failed, one intentional live-model ignore, normal test concurrency, 11.26s,
  exit zero. The live-model contract was separately executed at accepted source.

The same-head [Windows backend-free job](https://github.com/crussella0129/Animus_Ferric/actions/runs/33948474675/job/101258709904)
explicitly logged this quoting test passing; its CLI unit suite passed 318
tests in 9.15s. Different feature-matrix or local successes do not replace the
failed full Windows workspace gate.

## Diagnostic response, not an assumed fix

Reopen Test within this sprint and existing PR. Keep the locked plans, original
ten-second budget, literal quoting assertions and checked process cleanup.
Add bounded native-spawn timing and fixed script-entry/argument-completion
markers, so a recurrence identifies its stage without launching any ad-hoc
executable. Diagnostics alone do not establish the cause or resolve the blocker.
Fresh reviewed Test evidence and another extra post-Loop audit are required.

The initial diagnostic patch passed local `cargo test -p ferric-process --locked --quiet`
(nine passed, one source-mode ignore, 0.96s), normal-concurrency CLI units
(381 passed, one live ignore, 9.05s), warnings-denied process/CLI all-targets
clippy and formatting. Independent review confirmed unchanged deadlines,
authority and cleanup. It corrected a misleading `total_wall` label to
`execution_wall` because that measurement includes admission but excludes
checked cleanup. A fixed-size stage summary is retained on successful runs too;
bounded raw output remains failure-only. These are diagnostic checks, not a
resolution of C-002.

The diagnostic head `808cd9f0eb4651f3c56a84daca2dd79a66957a9d` passed all eight
jobs in both [push run 33949321009](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949321009)
and [PR run 33949323495](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949323495).
The four Windows quoting samples all passed with both markers observed:

| Sample | Execution wall | Native admission | CLI unit duration |
|---|---:|---:|---:|
| Push backend-free | 1.7584259s | 23.041ms | 7.34s |
| PR backend-free | 2.8406316s | 22.5581ms | 7.23s |
| Push full | 2.1699213s | 50.0987ms | 12.10s |
| PR full | 8.4410958s | 34.1342ms | 14.19s |

The slowest measured success used 84.4% of the original deadline. These success
samples do not recover the missing stage telemetry from either prior failure.

## Controlled-schedule mitigation

Independent review of locked E06 and the explicit startup race accepted a
bounded mitigation for qualification: align Windows's canonical workspace
command with Linux's existing `--test-threads=1` inter-test schedule. This
executes every test with its original deadlines and assertions. The product
startup race still creates two simultaneous worker threads, bounded barriers
and a retained winner lock inside its test; it is not serialized away.
The exact Windows command and matching operator documentation are ratcheted.

This is a controlled test schedule, **not a proven historical timeout cause or
parallel-suite robustness claim**. The original stages cannot be recovered
from logs that discarded them. T-12027 retains that separate investigation;
the new timing/marker diagnostics remain. C-002 stays blocked until all required
exact-head source/native/CI gates pass under this declared schedule. A recurrence
under isolation is a fresh blocker, not an invitation to retry or raise limits.

Independent source inspection also found that Windows admission counts any
non-error `ResumeThread` return as resumed, and accepts enumeration termination
without distinguishing exhaustion from error. Microsoft's
[resume contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-resumethread)
distinguishes previous suspend counts zero, one and greater than one; its
[enumeration contract](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-thread32next)
provides an exhaustion error coordinate. This is durable follow-up T-12026,
**not an established cause of these timeouts**. Do not repeatedly resume an
externally suspended thread to force it running.
