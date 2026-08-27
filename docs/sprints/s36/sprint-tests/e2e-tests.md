# Sprint 36 E2E Tests

**Status:** possible (via `--mock`, no real GGUF model required).

- `cli::mcp_stdio_e2e` (`crates/ferric-cli/tests/cli.rs`) — spawns `ferric mcp --mock` as a **real
  child process** (`std::process::Command`), writes JSON-RPC request lines to its stdin
  (`initialize` → `notifications/initialized` → a **malformed line** → `tools/list` → `tools/call`),
  and reads newline-delimited responses back from its stdout. Asserts: the initialize response
  carries a string `protocolVersion`; the malformed line gets back a `-32700` frame WITHOUT
  disrupting the requests around it (test-critique C-006); `tools/list` returns `ferric_query`;
  `tools/call` returns `mock run complete` with `isError:false`; and after stdin closes (EOF) the
  process **exits cleanly** (`status.success()`). This proves the real process framing — line
  delimiting, stdout purity, and error-then-recovery — not just the in-process dispatch logic the
  unit/integration tests cover. **Hardened against CI hangs** (test-critique C-007): stdout reads go
  through a background thread + `mpsc` channel, read via `recv_timeout(10s)` rather than an
  unbounded blocking `read_line`; stderr is drained on its own thread for the child's lifetime
  (an unread pipe can otherwise fill its OS buffer and deadlock the child); the final process-exit
  wait is a `try_wait()` poll bounded to 10s rather than a blocking `wait()`.

## Real-model E2E (manual, not automated)
A live llama-server backing `ferric mcp` over the OpenAI backend is a **manual verification step**,
consistent with the project's established no-live-backend-CI position (ADR-045 — CI can't depend on
live GGUF models). Not run this sprint; the `--mock` subprocess E2E above proves the transport and
lifecycle deterministically. The manual smoke, when run, would be:
`ferric server up --model <gguf>` then point an MCP client (or a hand-driven stdin) at
`ferric mcp --backend openai --model <name>` and issue one `ferric_query` call.

## Result
`mcp_stdio_e2e` passes (part of `cli.rs`'s 11). No E2E failures.
