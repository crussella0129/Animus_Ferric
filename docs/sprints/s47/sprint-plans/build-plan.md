Finalized - DO NOT EDIT

## Build Plan

### Increment 1: Golden Trace Testing
- Implement `ferric trace verify <golden.jsonl>` in `crates/ferric-cli/src/trace_verify.rs`.
- It will consume a `.jsonl` trace, build a deterministic `MockProvider` from its completion events, execute the loop, and verify identical output traces.

### Increment 2: Containerized E2E Harness
- Create `tools/run-e2e.sh` to launch the stack via Docker Compose, execute a known task (e.g., "create a file called e2e.txt"), and verify success.

### Increment 3: Coverage Scripts
- Create `tools/run-coverage.sh` as a shell script wrapping `cargo tarpaulin` with failure gates.
