# Sprint 123 Meta

- **Sprint number:** 123
- **Book schema version:** 2
- **Start timestamp:** 2026-09-06T23:23:43Z
- **End timestamp:** 2026-09-06T23:53:13Z
- **Model:** claude-opus-4-8
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Extract ferric-cli into a library with thin binary shims — the first decomposition increment (INT-0009 AC-1), which also removes the duplicate-source build warning (INT-0008 T-12028) by giving the two binaries distinct source files over a shared library.
- **Intents:** [INT-0009](../../intents/INT-0009-lean-decomposed-architecture.md) — planned; AC-1 (library extraction). [INT-0008](../../intents/INT-0008-unified-local-model-workflow.md) — active; T-12028 closed as a consequence.
- **Completion evidence:** ferric-cli library extraction (INT-0009 AC-1): lib + thin ferric/ferric-lifecycle-test shims + set-once bin_identity gate; duplicate-source warning gone (T-12028); full workspace green at d038ec6 (40 suites, 0 failed), lifecycle-fixture 5/5, Test clean
