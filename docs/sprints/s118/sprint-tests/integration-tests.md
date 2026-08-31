# Sprint 118 Integration Test Results

- **Tested code head:** `0145e45cb3ab8ab74ae71981d0851525eef2eb1c`
- **Primary command:** `cargo test -p ferric-cli --all-features server::tests`
- **Result:** 73 passed, 0 failed, 0 ignored.

## Deterministic lifecycle composition

`tailscale_fault_seam_clause_matrix` passed and invokes the named clause
matrices for pre-mutation launch failure, post-journal compensation, truthful
status, proxy-first down, ambiguous proxy/revision failure, idempotent retry,
and legacy-record blocking. Its scripted effect ledger asserts ordering rather
than merely terminal success:

- mirrored journal writes precede exact apply;
- entropy and identity preparation failures return before the production
  launch closure can advance its observable engine-spawn seam;
- active proxy comparison/off/absence verification precede process inspection
  and every signal attempt;
- independently authorized process cleanup continues when external state is
  ambiguous, while registration removal does not;
- both registration-revision checks carry captured non-empty revisions;
- replacement, duplicate, malformed, unreadable, off-error, and post-off
  unreadable states retain exact coordinate evidence and retry guidance;
- boolean-only Tailscale records in local, global, and promised-origin scopes
  invoke neither process nor Tailscale effects.

The event-ledger composition test is the authoritative proof that no process
signal attempt occurs before proxy cleanup. The cross-process fixture can prove
that the listener remains HTTP-live at successful scoped off, but cannot
observe a hypothetical failed pre-off signal attempt that left the listener
alive.

## Regression boundary

The all-feature workspace gate passed 1,108 tests with five intentional helper
ignores. It covers additive registration-schema parsing, lifecycle resolution,
conditional publication/removal, retained process handles, Windows listener
ownership, CLI parsing, the default run target, and template hygiene. This
supports the claim that typed Tailscale authority is additive and does not
weaken non-Tailscale or historical-record behavior.

Local path absolutization failure is not injected as an individual
cross-platform fault row because `std::path::absolute` has no deterministic
seam or portable invalid-path coordinate. This is an accepted deviation from
the finalized test-plan description, not a passed row. The governing EARS
outcomes name capture/publication/Serve/process/listener/revision behavior, all
of which is exercised; no Sprint 118 success claim depends on forcing that
standard-library call to fail.
