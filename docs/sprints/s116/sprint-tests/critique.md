# Test Critique — Sprint 116

## Concerns

### C-001: Planned EARS test names are not executable tests

- **Where:** `sprint-plans/test-plan.md` Intent Traceability and
  `sprint-tests/unit-tests.md` CLI test suites
- **Quote:** "`registration_inventory_retains_both_scopes_and_raw_bytes`"
- **Failure mode:** EARS-coverage
- **Why it matters:** Test enumeration on the merged tree found only one exact
  executable name among the nineteen EARS traceability names:
  `model_free_server_lifecycle_fixture_e2e`. Aggregate counts never remap
  E01-A through E05-D to the actual executed Rust tests, so clause-level proof
  is absent.
- **Suggested response:** add-test

### C-002: Concurrent lifecycle safety was not exercised

- **Where:** `build-plan.md` E01-C and `test-plan.md`
  `concurrent_lifecycle_operations_are_per_path_safe`
- **Quote:** "two Ferric processes attempt registration inventory,
  publication, adoption, or cleanup concurrently"
- **Failure mode:** negative-path
- **Why it matters:** Neither planned concurrency test exists. Implemented
  store tests are sequential or callback-simulated, and the CLI fixtures run
  sequential scenarios; none races two Ferric processes across shared paths.
- **Suggested response:** add-test

### C-003: Retained-process race promises lack adversarial execution

- **Where:** `test-plan.md` E02-A/E02-C retained-process and spawned-child
  matrices
- **Quote:** "Injected exit/PID reuse before binding, during readiness, before
  publication, and on every launch-failure cleanup"
- **Failure mode:** negative-path
- **Why it matters:** The named tests do not exist. Current platform tests
  cover parsers and happy-path native smoke, but do not force PID remapping or
  exit at each promised boundary and observe zero signal to a replacement.
- **Suggested response:** add-test

### C-004: Status and legacy-guidance output contracts are not asserted

- **Where:** `build-plan.md` E03-B/E04-E and their named test-plan rows
- **Quote:** "status ... SHALL enumerate local and global scope,
  process/listener identity, health, stale/conflict/unverifiable diagnostics,
  and the next safe action"
- **Failure mode:** weak-assertion
- **Why it matters:** Existing unit paths principally assert exit codes and the
  E2E success helper asserts process success. The legacy E2E adopts directly
  without first proving copy/paste-complete recovery guidance from status/down.
- **Suggested response:** tighten-assertion

### C-005: Teardown and publication failure matrices are missing

- **Where:** `test-plan.md` E04-B/E04-C/E05-A/E05-B
- **Quote:** "Injected terminate error, wait error, and wait timeout prove no
  published registration is rolled back without retained generation exit"
- **Failure mode:** negative-path
- **Why it matters:** Store-level byte-preservation tests cover only part of
  the contract. No executed lifecycle test injects terminate/wait failure,
  lingering listener, short write/sync/directory-sync failure, child exit
  during mirrored publication, or compensation failure while proving recovery
  registrations remain.
- **Suggested response:** add-test

### C-006: Tailscale evidence claims behavior the fixture does not check

- **Where:** `test-plan.md` E05-D and
  `server_lifecycle_fixture.rs::tailscale_refusal_has_zero_external_effects`
- **Quote:** "Doctor reports BLOCKED. A `tailscale: true` fixture blocks
  status/down cleanup before PID inspection for both present and absent PIDs"
- **Failure mode:** weak-assertion
- **Why it matters:** The E2E proves only `server up --tailscale` pre-side-effect
  refusal. It does not invoke doctor or exercise status/down for both live and
  absent captured Tailscale PIDs with observable inspection/reset boundaries.
- **Suggested response:** add-test

### C-007: Result provenance lacks the tested head and CI record

- **Where:** all Sprint 116 test artifacts and T-11504 completion evidence
- **Quote:** "passed on the final tree"
- **Failure mode:** evidence-drift
- **Why it matters:** The original artifacts named no exact command, tested
  SHA, or authoritative CI run. PR run `33294229347` tested
  `d450a755236c100fd1d9f67b2511435465a08989`; post-merge run `33320491690`
  tested `e6439b1eb4851d2262b6d1be973ff3098e65c3a4`. Both were green, but their
  default-feature workspace tests do not execute the feature-gated lifecycle
  fixture. The pass report was also written before this required critique.
- **Suggested response:** tighten-assertion

### C-008: The E2E retains a port/readiness flake risk

- **Where:** `server_lifecycle_fixture.rs` `unused_port`/`wait_until` and the
  unit-result reliability history
- **Quote:** "The test harness was corrected with a test-only mutex around
  those parent tests."
- **Failure mode:** flake-risk
- **Why it matters:** That mutex does not cover the integration fixture. Each
  E2E releases a port-zero reservation before the child binds and uses a fixed
  ten-second poll, leaving a separate unmitigated release-then-bind and
  fixed-deadline risk in parallel fixture execution.
- **Suggested response:** tighten-assertion

## Confidence

block
