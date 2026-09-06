# Test Critique — Sprint 121

## Concerns

### C-001: Canonical Windows acceptance remains unproved

- **Where:** `sprint-plans/build-plan.md` E04-C; `sprint-tests/ci-checkpoint-001.md`; INT-0008 AC-6/12 and Sprint 120 progress.
- **Quote:** "fresh source-level focused/native tests and the canonical final-head CI matrix SHALL pass"; "A recurrence under the canonical schedule is a blocker."
- **Failure mode:** EARS-coverage
- **Why it matters:** Source `2856c63209865f69b3d3727f84fd92f63f9dfa51` failed the actual first-run journey under the canonical Windows schedule. The 32-journey diagnostic sample did not reproduce or explain it; an instrumented green run alone cannot establish a repair or erase that failure.
- **Suggested response:** add-test — retain the original failure, resolve any evidenced fixture defect with targeted negative/positive regression coverage, and qualify the resulting immutable source through the unchanged gates. Keep any historical-cause uncertainty explicit. No accepted Test report or Loop close yet.

### C-002: A connection-level accept error ends the entire human fixture engine

- **Where:** `crates/ferric-cli/src/human_journey_tests.rs`, `fixture_human_engine`; E04-A/C human-front-door preservation.
- **Quote:** `exit_reason = "accept_error"; break;`
- **Failure mode:** flake-risk
- **Why it matters:** Only `WouldBlock` continues the accept loop. Microsoft documents that `accept` may return `WSAECONNRESET` when a queued peer terminates before acceptance—a connection-level event, not proof that the listening engine must stop. The current fixture then exits normally, potentially converting a recoverable peer event into "owned engine exited" during preparation or use. This establishes a fixture weakness, not the cause of the original CI failure. [Microsoft accept contract](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept).
- **Suggested response:** add-test — deterministically inject the documented reset through the actual fixture accept loop, then prove a subsequent valid request is served under the original absolute lifetime and checked cleanup. Make only narrowly justified connection-level errors recoverable; retain a fatal-error negative control. Do not add production identity retries or expand lifecycle authority.

## Confidence

block
