# Test Critique — Sprint 36

(Critic: foreground `Agent` tool, adversarial review against `prompts/test-critic.md`.)

## Concerns

### C-001: `files`-routing test covers only 1 of the 3 `Attachment` branches the locked plan demanded
- **Failure mode:** EARS-coverage
- **Response:** **add-test.** Replaced `tools_call_files_route_through_attach_fold_skip` with
  `tools_call_file_text_folds_into_prompt` (AppendText) and `tools_call_file_media_skipped_with_reason`
  (Skip — the security-relevant branch: an undeclared media file must not silently attach). The
  `Media`-successfully-attaches case is not separately added: `--mock` hardcodes
  `caps.supports_media = false`, so `decide_attachment` routes ANY media file to `Skip` regardless
  of `declared` — the same is true of the existing CLI test suite (no "media attaches" test exists
  there either), so this is parity with, not a regression from, prior coverage. Noted explicitly in
  the new test's doc comment rather than left implicit.

### C-002: The `files`-routing test's sole assertion (`isError:false`) can't distinguish "folded in" from "silently ignored"
- **Failure mode:** weak-assertion
- **Response:** **tighten-assertion.** `tools_call_file_text_folds_into_prompt` now reads the
  freshly written `mcp-*.jsonl` trace's `prompt_assembled` event and asserts its `chars` count is
  at least the injected file's length — mirroring `cli.rs`'s `query_file_text_folds_into_prompt`.

### C-003: T-3602's sampling values are named in the EARS clause but never asserted
- **Failure mode:** EARS-coverage
- **Response:** **add-test.** `run_config_matches_inline_computation` now asserts
  `config.sampling.temperature == a.temperature` and `config.sampling.max_tokens ==
  expected_policy.max_output_tokens`; `base_run_config_args()`'s temperature was changed from `0.0`
  (== `SamplingParams::default()`, a vacuous check) to `0.7` so the assertion is meaningful.

### C-004: No assertion that stdout stays frame-pure (only `eprintln!`, never `println!`)
- **Failure mode:** weak-assertion
- **Response:** **tighten-assertion**, via a cheap static check rather than a full stdio-capture
  test: `no_bare_println_in_source` scans `mcp.rs`'s own source (`include_str!`) and fails if any
  line starts with `println!`. Source inspection during the critique also confirmed the module
  uses only `eprintln!` today.

### C-005: Unknown-*method* negative path (`-32601`) is defined and dispatched but never tested
- **Failure mode:** negative-path
- **Response:** **add-test.** `dispatch_unknown_method_is_json_rpc_error` sends an unrecognized
  method and asserts `error.code == METHOD_NOT_FOUND` with no `result` — distinct from the existing
  unknown-*tool*-name test (`INVALID_PARAMS`, a different code path).

### C-006: No process-level negative-path E2E — the subprocess is never sent a malformed line
- **Failure mode:** e2e-cop-out
- **Response:** **add-test.** `mcp_stdio_e2e` now writes an invalid-JSON line between
  `notifications/initialized` and `tools/list`, asserts the real subprocess returns a `-32700`
  frame, and that the following `tools/list`/`tools/call` still succeed — proving the "keeps
  serving after a malformed line" property through the actual stdin→stdout pipe, not just the
  in-process `parse_line` unit test.

### C-007: `mcp_stdio_e2e`'s unbounded blocking `read_line` + undrained stderr pipe is a hang/flake risk
- **Failure mode:** flake-risk
- **Response:** **tighten (hardened, not deferred).** Reads now go through a background thread
  forwarding lines over an `mpsc` channel, read on the test thread via `recv_timeout(10s)` — a
  server that stops responding now fails the test instead of hanging CI. Stderr is drained on its
  own thread for the child's lifetime, removing the OS-pipe-buffer deadlock coupling. The final
  `child.wait()` was replaced with a `try_wait()` poll bounded to 10s for the same reason.

## Confidence

**proceed-with-caveats → all 7 concerns addressed (6 fixed/tightened, 0 deferred, 0 rejected).**
`cargo test --workspace` green (46 sprint-36-related tests total: 42 `ferric-cli` lib + the
subprocess E2E, up from the original 17); `clippy --workspace --all-targets -- -D warnings` and
`fmt --check` clean. Ready to finalize `test-report.md`.
