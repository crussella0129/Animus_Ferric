# Test Critique — Sprint 118

## Concerns

### C-001: Engine-spawn ordering is not test-observable
- **Where:** `build-plan.md` T-11801-E04; `test-plan.md` / T-11801 adapter and schema; `crates/ferric-cli/src/tailscale_serve.rs` `ownership_entropy_failure_precedes_side_effects`; `crates/ferric-cli/src/server.rs` `tailscale_pre_mutation_failures_never_apply`
- **Quote:** "entropy or identity failure SHALL precede engine spawn and every Serve mutation."
- **Failure mode:** weak-assertion
- **Why it matters:** The named tests call token/ownership preparation directly and inspect only the Tailscale-effect ledger. They have no engine-spawn seam or invocation marker, so moving engine spawn ahead of entropy or identity preparation in production `up` would not fail them despite the test plan claiming that neither engine nor Serve counters advance.
- **Suggested response:** add-test — exercise the production ordering through an injectable spawn effect or real-CLI engine invocation marker, asserting zero spawn and zero mutation for entropy and identity preparation failures.

### C-002: “Repeated down” bypasses discovery and the CLI path
- **Where:** `unit-tests.md` T-11803-E04; `crates/ferric-cli/src/server.rs` `down_retries_absent_proxy_and_stale_process`
- **Quote:** "already-absent, crashed/exited, and repeated down converge without broad mutation."
- **Failure mode:** weak-assertion
- **Why it matters:** The absent-proxy and already-exited rows each execute one manually constructed `DownPlan`, while the “repeated” assertion directly supplies `DownPlan::Empty`. It does not repeat discovery/planning or invoke a second real `server down`, so a regression that mishandles the state left by successful cleanup could still pass.
- **Suggested response:** add-test — invoke `server down` twice in the stateful CLI fixture, then assert the second invocation succeeds without Tailscale mutation, process signalling, journal recreation, or unrelated-state change.

### C-003: Frozen local-path negative row was not executed
- **Where:** `test-plan.md` / `tailscale_pre_mutation_failures_never_apply`; `integration-tests.md` / Regression boundary
- **Quote:** "capture/collision/readiness/identity/listener/local-path/publication fault table"; "Local path absolutization failure is not injected as an individual cross-platform fault row"
- **Failure mode:** negative-path
- **Why it matters:** The finalized plan explicitly promised a local-path failure row, but the result ledger omits it. The standard-library call’s lack of a seam explains the omission but does not make the frozen row executed.
- **Suggested response:** defer-with-rationale — record this as an explicit accepted plan deviation in the critique and final report, including why no affected EARS outcome depends on forcing that standard-library error; otherwise introduce a path-resolution seam and test cleanup/no-mutation behavior.

### C-004: One frozen verification command is red
- **Where:** `test-plan.md` Frozen Commands item 7; `unit-tests.md` / Frozen and regression commands
- **Quote:** "`cargo test -p ferric-cli --doc` | not applicable and exited 1"
- **Failure mode:** evidence-drift
- **Why it matters:** The supplemental workspace doc-test pass does not turn the immutable package-specific command into a pass. This is not a demonstrated product defect because `ferric-cli` is binary-only with doctests disabled, but the sprint cannot claim every frozen command succeeded.
- **Suggested response:** defer-with-rationale — retain the exact failure, binary-target metadata explanation, and supplemental workspace result in the final report, and characterize Test confidence as caveated rather than silently rewriting the gate.

## Confidence
block
