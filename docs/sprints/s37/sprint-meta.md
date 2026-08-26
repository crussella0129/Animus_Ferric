# Sprint 37 Meta

- **Sprint number:** 37
- **Start timestamp:** 2026-07-03T18:17:07Z
- **End timestamp:** 2026-07-03T21:05:00Z
- **Model:** claude-sonnet-5
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** **Streaming inference — fills ADR-003's reserved `complete_stream` extension point.**
  User-chosen focus, framed as "a base architectural choice." The core design tension:
  `ConstrainedJson` (the flagship path) returns every turn's completion — including the final
  `task_complete` answer — as one opaque JSON object, so raw token deltas aren't human-readable.
  Solved with `ConstrainedJsonScanner` (pure, thoroughly tested): an early "which tool" activity
  signal plus, only for `task_complete`, the live-decoded `summary` text, correctly handling JSON
  string-escape sequences (incl. multi-byte `\uXXXX`) across arbitrary chunk boundaries.
  `Provider::complete_streaming` ships with a default implementation (every non-overriding
  provider — mock, mistral.rs — behaves identically to `complete()`, zero code change);
  `OpenAiProvider` gets a real SSE implementation via `Response::chunk()` (discovered mid-build to
  need no cargo feature or extra dependency, simpler than the originally-planned `bytes_stream()` +
  `futures_util::StreamExt`). `RunArgs.stream_sink` threads through the loop (`None` = byte-identical
  to today); `ferric query --stream` is the CLI opt-in, skipping the final echo when streaming
  already displayed the answer. Six build tasks (T-3701–T-3706), all shipped. Two foreground
  critics ran (plan-phase: 10 concerns, all fixed/rejected-with-reason; test-phase: 5 concerns, 4
  fixed incl. a REAL bug — a malformed `\u` escape could have stalled the live display forever,
  caught and fixed before this report, not just a coverage gap — 1 deferred as locked-plan-vs-
  shipped-reality drift). Scope deliberately bounded: MCP streaming, mistral.rs backend streaming,
  seamless mid-stream retry, and a structured/programmatic streaming mode are explicit follow-ons,
  not silently dropped. 38 tests in `ferric-provider --features backend-openai` (up from ~9
  pre-sprint), 22 in `ferric-loop`, 42 in `ferric-cli`; `cargo test --workspace` green; clippy
  `-D warnings` (both feature sets) + fmt clean. One PR per sprint; `dev` clean.
