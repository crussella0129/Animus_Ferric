# Sprint 127 — Integration tests

All pass.

- `crates/ferric-gate/tests/layer.rs` (axum, `tower::ServiceExt::oneshot`):
  - Admitted requests reach the handler without the credential.
  - Denials carry `x-ferric-gate` and never a CORS header.
  - The fallback is gated too.
  - The health check is open but still Host-checked.
- `crates/ferric-valve/tests/gate.rs` (a real loopback valve in record-only
  mode in front of an upstream that records the `Authorization` it receives):
  - Refusals never reach the upstream. This covers no token, the gated
    `/slots` pass-through, and a drive-by `text/plain` POST with an `Origin`.
  - The upstream sees its own key (`Bearer upstream-secret`) and never the
    gate token.
  - Without an upstream key, nothing is forwarded.
- `ferric api --mock` on loopback, probed by `ferric secure probe` with the
  token file: 9/9 rows pass, the token file is mode 0600, an unauthenticated
  query gets `401 missing-token`, and an authenticated query runs.
  `--host 0.0.0.0` is refused at startup.
