# Sprint 122 Integration Tests

Tested head: `9eabcbc`.

## Probe reads real host memory
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) (AC-13, measured reading)
- `native_probe_reports_positive_total` (`crates/ferric-cli/src/startup/memory.rs`) — `NativeMemoryProbe.probe()` on the running host returns `Some(SystemMemory)` with `total_bytes > 0` and `available_bytes > 0`. Gated to `target_os = "linux"`/`"windows"`, so it asserts the real FFI (`GlobalMemoryStatusEx`) / `/proc/meminfo` read on both CI hosts and the dev host, complementing the pure `parse_meminfo` unit tests. **PASS** on Windows (dev host); Linux + Windows exercised by CI at the Loop phase.

This is the one place the platform boundary is crossed for real; every other
fit decision is a pure function fed a known number, so the suite has no timing,
retry, clock, randomness, or external-service dependence.
