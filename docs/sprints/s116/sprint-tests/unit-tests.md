# Sprint 116 Unit and Static Test Results

**Status:** passed on the final tree.

## CLI test suites

| Gate | Result | Evidence boundary |
| --- | --- | --- |
| CLI without default features | 214/214 passed | Exercises the reduced feature surface, including the model-free lifecycle implementation. |
| CLI with all features | 220/220 passed | Repeated three consecutive times after the test-only reliability fix; every final-tree run passed. |

The lifecycle coverage includes two-scope inventory, schema-v1/v2 authority,
conditional registration removal, no-clobber publication and rollback,
cross-workspace resolution, exact process/listener binding, fail-closed
Tailscale handling, wildcard/public listener behavior, and legacy adoption.

## Reliability history

An initial all-feature run exposed a lifecycle helper readiness timeout while
the helper-owning parent tests ran in parallel. The test harness was corrected
with a test-only mutex around those parent tests. This did not change the
production lifecycle path. The final 220-test suite then passed three
consecutive times, so the timeout is retained here as resolved test-harness
history rather than hidden as a clean first attempt.

## Static and compile gates

- Strict Clippy with warnings denied: passed.
- Workspace formatting check: passed.
- `git diff --check`: passed.
- The lifecycle feature fixture compiled for x86_64 and AArch64: passed.
- An additional AArch64 all-features/all-targets build was attempted but could
  not compile `ring` because the environment lacks `aarch64-linux-gnu-gcc`.
  That broader cross-build was optional and is not a Sprint 116 acceptance
  failure; no AArch64 runtime execution is claimed.
