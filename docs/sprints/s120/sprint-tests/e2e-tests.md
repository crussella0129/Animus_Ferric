# Sprint 120 E2E evidence

## Build acceptance trials

Source later committed in T-12004 at `b4475d1` (subsequent synchronous signal
admission and literal recovery quoting are included in that commit). These
Build trials preceded those last two corrections; an exact-head live rerun
and CI remain formal Test gates. No application or medium-horizon claim.

The opt-in Cargo test was invoked with the existing Qwen2.5-Coder-7B-Instruct
Q4_K_M GGUF in the repository's `models` directory:

```powershell
$env:FERRIC_LIVE_MODEL = '<repository>\models\qwen2.5-coder-7b-instruct-q4_k_m.gguf'
cargo test --locked -p ferric-cli --bin ferric real_model_prepared_host_journey -- --ignored --exact human::enabled::tests::real_model_prepared_host_journey --nocapture --test-threads=1
```

Result: 1 passed / 0 failed; test lifetime 8.63 seconds. Observed elapsed session
8.6087786 seconds; Ready 6.5709607 seconds; first response 7.2599263 seconds.
Actual runtime reported `llama-server` version `10034 (505b1ed15)`.
Actual settings: CPU-only, context 4096, temperature 0, unqualified.
Provider model ID was the actual canonical GGUF path, not a fabricated default.

Every input, in order:

| Prompt | Input |
|---|---|
| Ask only, or allow file work here? | `ask` |
| Start the local model? | `y` |
| You | `Reply with exactly: Ferric is ready.` |
| You | `/quit` |

Two setup decisions. Output stated CPU memory cost/unmeasured fit, bounded
engine checking/loading, owned foreground closed on exit, and Ask-only no file
changes. Actual answer: `Ferric is ready.` The trace contained SessionStart,
actual model/runtime/settings/user provenance, the answer, and SessionEnd with
`answered`. `session` returned Ok only after checked ProcessTree cleanup;
subsequent workspace-lock reacquisition passed. The fixture's `checked_cleanup`
field is conservative: it is true only on whole-session success, not an
independent classification of cleanup when a request fails. No failed request
or cleanup is claimed in this successful run.

## Actual terminal interaction

A second, real PTY invocation used `cargo r -- run --workspace <fresh-temporary-folder>
--model <repository-model>` on Windows. It was not a non-TTY welcome or merely
a scripted IO reducer. The live terminal received `ask`, `y`, the same question,
and `/quit` through actual terminal input. Observed transcript (folder/model
paths replaced with documentation placeholders):

```text
Folder: <fresh-temporary-folder>
Ask only, or allow file work here? [Enter = ask / work / quit] ask
This starts a local CPU model and may use substantial memory. Resource fit is not measured.
Start the local model? [y/N] y
Checking the installed engine…
Loading the model with conservative CPU settings (not hardware-qualified)…
Ready: <actual-model-path> (owned foreground (closed on exit))
Ask only — no file changes. Type a question; /quit ends the session.
You › Reply with exactly: Ferric is ready.
Ferric is ready.
You › /quit
Closing session…
```

Cargo process exit: 0. Product source owns the engine and explicitly awaits
checked cleanup before returning success. No target artifact was tool-invoked,
no extra executable was created as an ad-hoc proof, and no manual process
termination repaired either run. PTY elapsed time includes operator/tool
interaction delays, so the automated live trial supplies measured latencies.

## Scope

The mock E06-A composition is the human source journey suite plus the startup
fault/borrow/concurrency suites, not a claim that one function independently
retests every boundary. Unit/integration evidence will map each assertion.
Full acquisition/calibration/resume, ordinary-host Linux positive authority,
whole-Work Git cancellation and model-built application qualification remain
the named INT-0007/INT-0008 follow-ups in the locked plan. Native Linux source
acceptance must pass the explicitly isolated CI environment.

## Formal Test live run at first CI head

Exact head: `8695b5066412f99abf909caacb58486223a25230`. The exact L invocation
in the integration map passed 1/1 in 10.90 seconds. Same existing
Qwen2.5-Coder-7B-Instruct Q4_K_M file and actual llama-server
`10034 (505b1ed15)`, CPU-only, context 4096, temperature 0, unqualified.
Session elapsed: 10.8772423 seconds; Ready: 8.928912 seconds; first response:
9.5977077 seconds. Inputs remained exactly `ask`, `y`,
`Reply with exactly: Ferric is ready.`, `/quit`. Actual answer remained
`Ferric is ready.` The trace ended `answered`; session result was Ok after
checked owned cleanup and workspace lock reacquisition passed. No manual
termination, download or capability promotion occurred.

This is a passing live result, not a passing overall Test verdict: that head's
required ARM64 CI gate failed for a missing cross C compiler. The CI correction
and final acceptance must remain explicit; this earlier live result is not
silently rebound to a later commit.

## Formal Test live run at second CI head

Exact head: `6635164fdcc1205f7afc2d64babe90fb98261b16`. L passed 1/1 in 6.05
seconds using the same existing Qwen2.5-Coder-7B-Instruct Q4_K_M GGUF and actual
llama-server `10034 (505b1ed15)`, CPU-only/context 4096/temperature 0/unqualified.
Session elapsed 6.0246996 seconds; Ready 4.0623322 seconds; first response
4.7641066 seconds. Actual inputs: `ask`, `y`,
`Reply with exactly: Ferric is ready.`, `/quit`; actual answer `Ferric is ready.`.
Trace `human-be894675330a544dd6d30e341fc229ce` recorded SessionStart, actual
model/runtime/settings and answer, then SessionEnd `answered`. Source result
was Ok after checked owned cleanup; workspace lock reacquisition passed.
No manual termination or acquisition occurred. This live success does not
accept the two failing CI jobs at this head, nor bind evidence to a later head.

## Corrected CI candidate live run (before final copy review)

Exact head: `d3173ca40c2e3236080b0d7b1076728e0d5c682b`. The exact L command in
the integration map passed 1/1 in 5.84 seconds, with the existing repository
`models/qwen2.5-coder-7b-instruct-q4_k_m.gguf` and actual llama-server
`10034 (505b1ed15)`. CPU-only, context 4096, temperature 0; qualification remains
unmeasured prepared-host conversation only. No model was downloaded or replaced.

- Session elapsed: 5.8209369 seconds.
- Time to Ready: 3.9615075 seconds.
- Time to first response: 4.5857036 seconds.
- Decisions: Ask/work (`ask`) and local resource commitment (`y`). Explicit
  existing model selection avoids ambiguity; no technical settings were asked.
- Objective: `Reply with exactly: Ferric is ready.`
- Actual response: `Ferric is ready.`; then `/quit`.
- Trace: `human-79195ad14123f8f9abf2fcbd4394335c`, SessionStart followed by actual
  model/runtime/settings/user provenance and answer, SessionEnd `answered`.
- Result: `Ok`, `checked_cleanup: true`; source checked owned reaping and then
  workspace lock reacquisition passed. No manual process repair occurred.

The observed transcript included the substantial-memory/unmeasured-fit warning,
installed-engine check, conservative CPU loading progress, Ready with owned
foreground/closed-on-exit scope, Ask-only/no-file-changes mode, actual answer
and Closing session. This is short-response usability evidence, not model-built
application, Qwen3.8 qualification, hardware fit, or autonomous Sprint Loops
compatibility. Final native-terminal evidence follows separately.

## Corrected CI candidate native terminal (before final copy review)

At the same `d3173ca40c2e3236080b0d7b1076728e0d5c682b`, actual Windows PTY ran
`cargo r -- run --workspace <fresh-temporary-workspace> --model <existing-repository-model>`.
Only evidence prose changed since that commit; implementation, dependencies and
CI source were unchanged. Cargo compiled/ran the source; no artifact was invoked
directly by the agent. The terminal received real input, not the scripted IO seam:

```text
Folder: <fresh-temporary-workspace>
Ask only, or allow file work here? [Enter = ask / work / quit] ask
This starts a local CPU model and may use substantial memory. Resource fit is not measured.
Start the local model? [y/N] y
Checking the installed engine…
Loading the model with conservative CPU settings (not hardware-qualified)…
Ready: <existing-repository-model> (owned foreground (closed on exit))
Ask only — no file changes. Type a question; /quit ends the session.
You › Reply with exactly: Ferric is ready.
Ferric is ready.
You › /quit
Closing session…
```

Cargo/terminal exited 0 after source-level `prepared.cleanup()` returned success.
No manual termination was performed. The separate L fixture above additionally
asserts lock reacquisition after cleanup. PTY latency includes tool/operator
input delays and is not presented as measured inference performance. This
terminal run uses the production front door with an explicit isolated workspace
and model; plain zero-argument routing is separately proved by the manifest,
shared orchestration and non-TTY process assertions, not misreported as this argv.

## Final corrected implementation-head live acceptance

Exact head: `0ec5a0eb0f465e8220b7f2010428aed3d6f2975d`, after the independently
reviewed E04-D error-guidance correction. Exact L passed 1/1 in 5.96 seconds.
Actual runtime remained llama-server `10034 (505b1ed15)`, using the existing
repository `models/qwen2.5-coder-7b-instruct-q4_k_m.gguf`, CPU-only, context 4096,
temperature 0 and explicitly unmeasured capability. No downloads occurred.

Session elapsed 5.9389204 seconds; Ready 3.999448 seconds; first response
4.6735217 seconds. Actual input remained `ask`, `y`,
`Reply with exactly: Ferric is ready.`, `/quit`; actual answer `Ferric is ready.`.
Transcript retained the memory/CPU/unmeasured-fit warning, engine check, loading,
owned Ready, Ask/no-file-change scope and Closing. Trace
`human-03319677fdee0a4e1d6b081976b55002` recorded actual model/runtime/settings,
answer and terminal reason `answered`. Source result was `Ok` with
`checked_cleanup: true`, followed by successful workspace lock reacquisition.
No manual termination repaired the run. Prior candidate runs above remain
separately bound and are not substituted for this result.

### Final corrected native terminal

Actual Windows PTY at the same `0ec5a0eb0f465e8220b7f2010428aed3d6f2975d` ran
`cargo r -- run --workspace <new-isolated-folder> --model <existing-repository-model>`.
Observed terminal input/output was again `ask`, the CPU/unmeasured-memory
warning, `y`, Checking/Loading/owned Ready, then the objective
`Reply with exactly: Ferric is ready.`, streamed answer `Ferric is ready.`,
`/quit`, and `Closing session…`. The process exited 0 after source-owned checked
cleanup. Trace `human-822f8845f86e959640e74cd21f7371dd` independently retained
actual model/runtime/settings, the same answer and terminal reason `answered`.
No manual termination, target-artifact command or ad-hoc executable proof was
used. The explicit temporary folder/model isolate this terminal test; zero-arg
routing remains the separately named manifest/shared-orchestration evidence.
These are fresh observed results, not transferred candidate transcripts.
