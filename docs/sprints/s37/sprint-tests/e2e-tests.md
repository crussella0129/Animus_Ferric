# Sprint 37 E2E Tests

**Status:** possible (via a hand-rolled local TCP fake server + `--mock`, no real GGUF model or
external mocking dependency required).

- `ferric_provider::openai::streaming_e2e::complete_streaming_over_real_tcp_stream` — binds a real
  `tokio::net::TcpListener` on an ephemeral `127.0.0.1` port, accepts one connection, and writes a
  canned raw HTTP/1.1 response with an SSE body (`Content-Type: text/event-stream`,
  **`Connection: close`, deliberately not `Content-Length`** — an SSE body is unbounded-length, and
  a wrong/missing length would make `reqwest` hang waiting for more bytes instead of completing,
  per plan-critique C-009). `OpenAiProvider::complete_streaming`, pointed at that address, reads
  the response via `Response::chunk()` (the real production code path) and returns the correctly
  accumulated `Completion` (`"Hello world"`, not truncated) with the sink recording the exact
  expected `Text` delta sequence in order. Proves the real wire protocol — SSE line splitting
  across TCP chunk boundaries, `Connection: close` framing — not just the pure accumulator logic
  the unit tests cover. Feature-gated (`backend-openai`), `#[tokio::test]`.
- `cli::stream_flag_mock_no_duplication` (`crates/ferric-cli/tests/cli.rs`) — a real `ferric query
  --mock --stream` subprocess; asserts the final text appears in stdout exactly once (the
  `--mock` script's completions are native tool calls with no `message.text`, so the default
  `complete_streaming` fires zero deltas for either turn — the final echo is the only place the
  text appears, proving no duplication end to end through the real binary).
- `openai::streaming_e2e::complete_streaming_surfaces_http_error_status` (test-critique C-003) —
  the fake server returns a real `404` response; confirms `complete_streaming` surfaces
  `ProviderError::Backend` naming both the status and body, with zero deltas fired and no hang.
- `openai::streaming_e2e::complete_streaming_reassembles_a_line_split_mid_write` (test-critique
  C-005) — a single `data:` line's JSON is split across two separate socket writes with a real
  gap between them, forcing genuine mid-line TCP fragmentation (not incidental OS buffering);
  confirms the read loop's line-buffering correctly reassembles it. This is the one piece of new
  I/O logic the pure `feed_line` unit tests can't reach (they receive pre-split lines by
  construction) — the fake-server harness was purpose-built to prove exactly this.

## Real-model E2E (manual, not automated)
Watching text actually stream token-by-token against a live llama-server (rather than the
deterministic fake-server/mock paths above) is a **manual verification step**, consistent with the
project's established no-live-backend-CI position (ADR-045 — CI can't depend on live GGUF models).
Not run this sprint; the fake-server E2E above proves the transport/framing deterministically, and
the `ConstrainedJsonScanner`'s unit tests prove the extraction logic exhaustively (incl. escape
edge cases no live model run would reliably exercise on demand).

## Result
All 4 E2E tests pass (2 original + 2 added during the test-critic pass). No E2E failures.
