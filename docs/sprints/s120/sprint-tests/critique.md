# Test Critique — Sprint 120

## Concerns

### C-002: Historical timeout cause remains unknown; controlled scheduling is qualified

- **Where:** `checkpoint-diagnosis.md`; `ci-results.md` qualification candidate
  `4f4e4f0`; `query.rs::powershell_quote_round_trips_argv`; locked E06-B; T-12027.
- **Quote:** "This is a controlled test schedule, not a proven historical
  timeout cause or parallel-suite robustness claim."
- **Failure mode:** flake-risk | evidence-drift
- **Why it matters:** The two historical Windows timeouts remain unexplained;
  neither subsequent success nor serialization identifies their missing stage.
  Revised qualification preserves every test, the ten-second bound, exact argv
  assertions and checked cleanup, while controlling unrelated test scheduling
  and retaining explicit concurrent startup workers.
- **Suggested response:** defer-with-rationale — accept the controlled-schedule
  mitigation at `4f4e4f04d4ee132f9df9bb422be88a5ce366915d`, retain diagnostics
  and T-12027, and report the limitation prominently. Recurrence under this
  schedule becomes a blocker requiring investigation, not repeated retries or
  relaxed deadlines. T-12026 remains separate admission-hardening work, not an
  established timeout cause.

C-001 remains resolved. No additional blocking intent/EARS coverage or assertion
weakness was found within the locked partial-sprint scope.

Renewed independent review substantiates:

- Both exact-head CI runs, 33949875039 and 33949876363, passed all eight jobs
  without reruns. The 75-row native confirmation table independently sums to
  Windows 1,247 passed/seven intentional ignores and Linux 1,253/five, each run.
- Backend-free, lifecycle, formatting and warnings-denied clippy gates passed;
  ARM64 remains compile-only evidence.
- Canonical Windows scheduling is explicitly serialized. The startup test
  still races two workers through bounded barriers, retains the winning lock
  until both attempts finish, and asserts one launch and checked exit.
- Fresh source-owned live acceptance passed in 7.02s with the expected answer,
  answered trace, checked cleanup and lock reacquisition. Separate actual Cargo
  terminal interaction produced the answer and exited zero after `/quit`.
- The corrected integration map distinguishes current commands/results from
  historical focused invocations. Locked plans remain unchanged.

Acceptance is limited to Python maintenance, configuration admission and the
prepared-host human workflow. It does not establish acquisition/calibration,
complete resume, whole-Work cancellation, arbitrary parallel-suite robustness,
platform parity or medium-horizon application success.

## Confidence

proceed-with-caveats
