# INT-0007 — Hardware-calibrated autonomous development

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0007
- **State:** active
- **Work evidence:** [T-11407 through T-11413 build plan](../sprints/s114/sprint-plans/build-plan.md#execution-sequence)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 114 research](../sprints/s114/sprint-research/research-report.md)

## Intent

Prove what Animus Ferric can currently accomplish on this project's local
hardware by selecting a source-pinned GGUF that fits the host, using Ferric to
build a bounded multi-file application, and preserving enough evidence to
distinguish model behavior, harness behavior, runtime limitations, and grader
results.

The same evaluation must determine, layer by layer, whether Ferric can use the
current Animus Sprint Loops Book-v2 distribution. A repository's compatibility
claim is a hypothesis: discovery, authorization, resource access, helper
execution, Book advancement, local Git, re-entry, and remote checkpointing are
separate outcomes and must not be collapsed into a single success label.

The sprint is an evaluation and integration exercise. It does not authorize
Codex to repair the model-authored application, execute arbitrary model-authored
shell commands, weaken Ferric's operator-authorization boundary, or represent a
partial/manual Sprint Loop route as autonomous end-to-end support.

## Acceptance criteria

1. A current primary-source survey and measured host inventory select one exact
   GGUF, quantization, source revision or immutable hash, license, storage
   destination, and initial context/runtime settings. The artifact is stored
   under the repository's ignored `models/` directory and its local SHA-256 is
   checked against the publisher's value.
2. The selected model is smoke-tested through Ferric's actual managed
   `llama-server` path. Startup allocation, effective context, GPU offload,
   health/model identity, sampling settings, binary versions, and teardown are
   recorded; an engine incompatibility is classified separately from a model
   failure.
3. Ferric receives a frozen, independently gradable, medium-horizon Rust
   application task that requires repository inspection, planning, multiple
   source files, model-authored tests, verification-driven repair, and at least
   one continuation boundary. The prompt, seed hashes, checks, grader, model,
   binaries, invocations, traces, effects, final workspace, results, and
   teardown evidence are retained.
4. After the first Ferric invocation, Codex makes no edits to the candidate
   workspace. The grader runs model-authored code only inside a bounded,
   network-disabled sandbox; infrastructure failure and model failure remain
   distinct.
5. A pinned Animus Sprint Loops distribution is tested in an isolated
   workspace. Results separately report discovery, explicit authorization,
   top-level instruction injection, linked-resource access, router/helper
   execution, Book advancement, cross-run resumption, local Git, and remote
   checkpoint authority.
6. The closeout states the observed capability boundary without inflating
   trained context into usable context, publisher benchmarks into local
   performance, prompt injection into orchestration, or structural trace
   validity into application success.
7. The landing README no longer carries sprint-specific result history; it
   retains concise current semantics and routes readers to the authoritative
   Sprint Book, intents, current work, and completed work without altering the
   underlying historical evidence.

## Rationale

Sprint 113 showed that the existing Qwen2.5-Coder-7B control and an
evidence-bound controller both completed zero of three frozen long-horizon
tasks. The user asked for the next practical question: whether a stronger
current model that genuinely fits this machine can complete a realistic app
through Ferric, and whether Ferric's recently advertised Sprint Loops support
is operational rather than nominal. A pinned, instrumented trial answers both
without reopening the falsified Sprint 113 intervention.

## Alternatives

- Continue tuning the old 7B model: rejected for this sprint because the user
  explicitly requested a current model survey and a practical application run.
- Download the strongest agentic-coding GGUF regardless of resident size:
  rejected because active-parameter counts do not eliminate the need to store
  and map all weights.
- Let Codex finish or repair the application after Ferric stalls: rejected
  because it would destroy causal attribution.
- Treat `--skill` prompt injection as complete Sprint Loops support: rejected
  because Book v2 also requires resources, helpers, re-entry, Git, and remote
  authority.

## Consequences

The selected model consumes several gigabytes of ignored local storage and may
still run below its trained context ceiling. The trial requires a frozen grader,
sandbox setup, full trace retention, and deliberate teardown, so it is slower
than an informal demo. A negative or partial result is an acceptable realized
outcome if every acceptance boundary is tested and reported truthfully; product
changes discovered by the trial belong to follow-on intents.

## Transition history

- 2026-08-26: created as `proposed` from the user's request for a
  hardware-fitting current model, a Ferric-built application, complete run
  evidence, and a live Animus Sprint Loops compatibility verdict.
- 2026-08-26: moved from `proposed` to `planned`; Sprint 114 tasks T-11407
  through T-11413 freeze model acquisition, sandboxed grading, managed-server
  calibration, the no-repair app trial, the layered skill probe, README
  cleanup, and truthful closeout.
- 2026-08-26: moved from `planned` to `active` when Sprint 114 Build began with
  the frozen Qwen3.8-27B quant acquisition task T-11407.
