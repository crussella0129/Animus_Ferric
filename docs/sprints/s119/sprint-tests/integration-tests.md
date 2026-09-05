# Sprint 119 Integration Verification

## Retained first committed attempt

Head `712e3cc5eae19170601d3c3feaee4deab03bbbd4`:

- Local Windows workspace Cargo tests: 1,126 passed / 6 intentional ignores.
- Affected suites: bench 78/3 ignored, CLI unit 310/0, CLI integration 68/0,
  bench command integration 7/0, shared process 6/1 ignored, source ratchet 1/0,
  template hygiene 3/0. Real Python grading was explicitly enabled.
- [CI run 33934904691](https://github.com/crussella0129/Animus_Ferric/actions/runs/33934904691):
  **failure** overall; five jobs succeeded and isolated Linux lifecycle failed.
  Both platform workspace jobs, both feature lint matrices, Windows native
  lifecycle, and aarch64 compile checks passed within those jobs.
- Linux workspace execution explicitly passed parent watcher, invalid pidfd
  events, stale registration generation, serialized shutdown, inherited writer,
  timeout/success/unwind and bounded-capture assertions. These are actual Linux
  tests, not the aarch64 compile-only gate.

This head is superseded by the documented
[Test corrections](../test-phase-corrections.md); partial green gates do not
accept E04/E05/E07 while positive Linux lifecycle fails. Corrected-head native
CI and final clause confirmation must be recorded before Test acceptance.
