# Sprint 115 Meta

- **Sprint number:** 115
- **Book schema version:** 2
- **Start timestamp:** 2026-08-28T03:14:59Z
- **End timestamp:** 2026-08-30T03:00:29Z
- **Model:** Codex host model not exposed; evaluated local model Qwen3.8-27B UD-Q4_K_M
- **Exit status:** aborted
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Added a safe external query-trace root and qualified the exact post-reboot CUDA Qwen runtime. The immutable live handoff ended externally before the frozen no-repair MH-RS01 trial began, so no application success is claimed and the trial remains ordered follow-up.
- **Intents:** [INT-0007 — Hardware-calibrated autonomous development](../../intents/INT-0007-hardware-calibrated-autonomous-development.md); [INT-0008 — Unified local-model workflow](../../intents/INT-0008-unified-local-model-workflow.md)
- **Completion evidence:** [partial test report](sprint-tests/test-report.md); [managed-runtime attempt 002](control-artifacts/runtime/attempts/002/result.json); [external field-report adjudication](sprint-research/external-field-report-adjudication.md); [exact stale-registration cleanup](control-artifacts/runtime/cold-registration-cleanup.json)

## Partial outcome

T-11414, T-11501, and T-11502 completed. T-11503 achieved its qualification
portion but closed partial because its dependent-use clause remained unmet.
Attempt 002 started the exact Qwen3.8-27B UD-Q4_K_M coordinate at
context 32,768 through the qualified CUDA engine, proved 24 of 66 layers
offloaded, passed the constrained smoke, and measured a
3.565083339811294 decoded-token/s median across three scored samples. Its
64-file retained archive and handoff verify offline.

The attempt-002 server was later stopped outside the frozen Sprint 115
protocol before T-11410 consumed it. T-11410 was never started: no candidate,
app trace, grader result, or app-run manifest exists. T-11412 therefore remains
incomplete even though the final cold-state predicate was recovered.

## External exploratory evidence

An external agent subsequently reported a working three-turn counter app using
the same quant with a different CPU-only engine and context. It is encouraging
anecdotal evidence, but it is excluded from T-11410 and INT-0007 AC-3/4 because
it did not use the frozen task, continuation, sandbox, no-repair attribution,
effect reconciliation, or evidence manifest. Its code-backed product findings
and corrected GPU conclusion are recorded in the adjudication linked above.

## Cold closeout

All five exact disposable roots, the owned listener, and the global runfile
were absent. One stale local registration remained with the exact attempt-002
handoff hash. After rechecking its bytes, absent listener, absent global
registration, and non-live retained process identity, closeout removed only
that file and sent no process signal. Models, committed evidence, and retained
quarantine were preserved.

## Abort note (2026-08-30T03:00:29Z)

The qualified live handoff was terminated externally before dependent task
T-11410 could begin; Sprint 115 therefore closed aborted with partial results.
