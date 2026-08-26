# Test Critique — Sprint 37

(Critic: foreground `Agent` tool, adversarial review against `prompts/test-critic.md`, specifically
tasked with checking negative paths and consistency with the mid-build `chunk()` vs
`bytes_stream()` switch.)

## Concerns

### C-001: `ConstrainedJsonScanner` conflated "incomplete `\u` escape" with "malformed `\u` escape" — the latter could stall the live display forever
- **Failure mode:** negative-path
- **Response:** **fix (code + test).** `decode_json_string_prefix`'s `\u` hex-digit loop now
  distinguishes `None` (genuinely incomplete — more text may resolve it, hold back and wait) from
  `Some(non-hex-char)` (malformed — no amount of further text will ever complete a valid escape;
  stop decoding this field cleanly, as if the string had ended, rather than stalling). Added
  `malformed_unicode_escape_stops_gracefully`, confirming the scanner decodes the text before the
  malformed point and never hangs or re-emits on subsequent calls.

### C-002: No test for `complete_streaming` called with a request that fails `validate()`
- **Failure mode:** negative-path
- **Response:** **add-test.** `complete_streaming_rejects_invalid_request` pins that a
  both-tools-and-constraint request returns `Err(ProviderError::InvalidRequest(_))` with zero
  deltas fired and no network call — a future refactor can't silently drop or reorder the
  `validate()?` call ahead of the send.

### C-003: No test for `complete_streaming`'s non-2xx HTTP-status path
- **Failure mode:** negative-path
- **Response:** **add-test.** `complete_streaming_surfaces_http_error_status` (real fake-server
  E2E) confirms a `404` response surfaces as `ProviderError::Backend` carrying both the status and
  the body text, with no delta fired and no hang.

### C-004: `test-plan.md`'s T-3703 E2E section still describes the superseded `.bytes_stream()` mechanism, not the shipped `Response::chunk()` approach
- **Failure mode:** EARS-coverage (documentation/artifact-consistency)
- **Response:** **defer-with-rationale** — `test-plan.md` is locked (`Finalized - DO NOT EDIT`,
  matching `build-plan.md`'s own historical `bytes_stream()` references, which the critic
  correctly treated as "already-addressed history" from before the build-time discovery). The
  actually-authoritative, unlocked artifacts — `sprint-tests/e2e-tests.md`, ADR-047, and the source
  itself — all correctly describe `Response::chunk()`. Recorded here explicitly (per the critic's
  own instruction) rather than silently absorbed: a reader diffing the locked `test-plan.md`
  against `Cargo.toml`/the source will find the `stream` feature absent and the mechanism
  different: this is expected drift between a pre-build plan and the as-built reality, not a
  currently-wrong claim about what shipped.

### C-005: No test forces genuine mid-line TCP fragmentation (the one thing the fake-server harness exists to prove)
- **Failure mode:** e2e-cop-out
- **Response:** **add-test.** `complete_streaming_reassembles_a_line_split_mid_write` splits a
  single `data:` line's JSON across two separate socket writes (with a 20ms gap), forcing the
  `buf`/`drain`/`find('\n')` buffering logic in `complete_streaming`'s read loop to actually
  reassemble a fragmented line — the one piece of new I/O-adjacent logic the pure `feed_line` unit
  tests structurally can't exercise (they receive pre-split lines by construction).

## Confidence

**proceed-with-caveats → all 5 concerns addressed (4 fixed with new tests/code, 1 deferred with
explicit rationale — a locked-plan-vs-shipped-reality drift, not a live defect).**
`cargo test -p ferric-provider --features backend-openai`: 38 passed (up from 34 — 4 new tests:
the malformed-escape fix, the validate-rejection test, the HTTP-error-status E2E, and the
mid-line-fragmentation E2E). `cargo test --workspace` green; clippy `-D warnings` (both feature
sets) + fmt clean. Ready to finalize `test-report.md`.
