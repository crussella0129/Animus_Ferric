# Sprint 37 Test Report — Streaming inference (ADR-047)

## Summary
- Unit tests: 18 passed / 0 failed / 18 total (`ferric-provider` default features — includes
  `stream_scan`'s pure escape-decoding suite, the sprint's core novel logic).
- With `--features backend-openai`: 38 passed / 0 failed / 38 total (adds `OpenAiProvider`'s SSE
  accumulation, validation, and error-path tests).
- `ferric-loop`: 22 passed / 0 failed / 22 total (incl. 2 streaming integration tests; every
  pre-existing test file passes unchanged, proving the `stream_sink: None` regression clause).
- `ferric-cli`: 42 passed / 0 failed / 42 total (default); unaffected under `--features
  backend-openai`.
- E2E: 4 passed / 0 failed / 4 total (2 real-TCP-socket tests against `OpenAiProvider`, 2 real
  subprocess tests against the `ferric` binary).
- Full workspace (`cargo test --workspace`): **all crates green**, no regressions in any
  previously-shipped sprint's tests.
- Lint/format: `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo clippy -p
  ferric-provider --features backend-openai --all-targets -- -D warnings` clean; `cargo fmt --all
  --check` clean.
- CI status: not run this sprint (`.github/workflows/ci.yml` exists; runs on push/PR as usual).

## Failures
None persisted. The test-critic's C-001 finding (a genuinely malformed `\u` escape could stall the
`ConstrainedJsonScanner`'s live display forever, since "ran out of input" and "got a non-hex
character" were conflated into the same code path) was a **real bug**, not just a coverage gap —
fixed in `crates/ferric-provider/src/stream_scan.rs` before this report was written, with a
regression test (`malformed_unicode_escape_stops_gracefully`). No other failure occurred at any
point in this sprint's Test Phase.

## Technical Debt Identified
- **`test-plan.md`'s locked T-3703 E2E section still describes `.bytes_stream()`**, the mechanism
  originally planned before a build-time discovery (`Response::chunk()` needs no cargo feature or
  extra dependency, simpler than `bytes_stream()` + `futures_util::StreamExt`) changed the actual
  implementation. `test-plan.md` is `Finalized — DO NOT EDIT`, so this is left as documented drift
  between the pre-build plan and as-shipped reality (see `critique.md` C-004) rather than edited —
  the authoritative, unlocked artifacts (`e2e-tests.md`, ADR-047, the source itself) are all
  consistent and correct.
- **MCP streaming, mistral.rs backend streaming, seamless mid-stream retry, and a structured
  programmatic streaming mode** remain explicit, ADR-047-recorded follow-ons — not attempted this
  sprint, not silently dropped.
- **A malformed (not merely incomplete) escape in `task_complete`'s summary stops the LIVE display
  at that point** (the C-001 fix's graceful-stop behavior) even though the final, fully-parsed
  answer (via the normal non-streaming-equivalent completion text) is unaffected — a narrow,
  accepted display-only rough edge, parallel to the already-accepted truncation-mid-summary edge
  case from the plan phase (`build-plan.md` T-3702 notes).

## Coverage Observations
- Every EARS clause in the locked `build-plan.md` now has a corresponding test, including the
  plan-critique's own hardening (multi-byte `\uXXXX` escape splits, the false-positive-safety
  regression, retry-duplication) and the test-critique's negative-path additions (malformed
  escapes, request validation failure, HTTP error status, genuine mid-line TCP fragmentation).
- The sprint's riskiest piece of new logic — an incremental JSON-string decoder handling arbitrary
  chunk-boundary splits — is now exhaustively tested at every escape-sequence cut point, both for
  legitimately incomplete input (wait for more) and genuinely malformed input (stop cleanly,
  discovered and fixed as a real bug during the test-critic pass, not merely a coverage gap).
- The fake-TCP-server E2E harness (introduced for the happy path) was reused for two more real
  wire-protocol proofs (an HTTP error response, a deliberately fragmented line) rather than left as
  a single-purpose test — validating the harness's own value beyond its first use.
- `stream_sink: None`'s "byte-identical to today" claim is proven for free by every OTHER test in
  `ferric-loop` and `ferric-cli` continuing to pass unmodified after the field's addition — the
  strongest form of regression evidence available (no test had to be rewritten to account for the
  new capability existing).
