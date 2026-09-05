# INT-0007 — Hardware-calibrated autonomous development

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0007
- **State:** active
- **Work evidence:** [Sprint 114 T-11407 through T-11413](../sprints/s114/sprint-plans/build-plan.md#execution-sequence); [Sprint 115 continuation plan](../sprints/s115/sprint-plans/build-plan.md#execution-sequence); [Sprint 115 partial closeout](../sprints/s115/sprint-tests/test-report.md); [stable ordered calibration and workflow backlog](../work/tasks.md#post-sprint-115--ordered-local-model-work); [Sprint 121 approved explicit-budget plan](../sprints/s121/sprint-plans/build-plan.md#execution-sequence)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** [Sprint 115 release, harness, and managed-runtime evidence](../sprints/s115/sprint-tests/test-report.md)
- **Documentation evidence:** [Sprint 114 research](../sprints/s114/sprint-research/research-report.md); [external field-report adjudication](../sprints/s115/sprint-research/external-field-report-adjudication.md); [Sprint 116 lifecycle and wider-gap research](../sprints/s116/sprint-research/research-report.md)

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

The durable product direction is to preserve the constrained-JSON harness as
a viable substrate and recalibrate the operating envelope around it, rather
than replace the guard, action protocol, trace/replay, or compaction
architecture. The external Qwen3.8 field report is sufficient to motivate that
direction, but it is not repository-native completion evidence: its successful
three-turn application and no-grammar-hang observation must still be reproduced
under the frozen attribution and grading boundaries above.

Calibration is a first-class runtime capability, not a static parameter-count
table. Animus Ferric should detect available accelerated backends and hardware,
warn when requested GPU offload is unavailable or inert, choose conservative
hardware-aware defaults, and persist measurements tied to the exact model,
quantization, engine, build, and host class. Modern context, reasoning behavior,
generation speed, action size, and compaction cost all participate in the
effective profile; every automatic choice remains visible and overridable.

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
8. A repository-native frozen trial confirms or rejects the external report's
   harness-viability observation by retaining constrained-JSON actions, model
   and backend identity, traces, an independent grader, and grammar/transport
   diagnostics. One successful external application may select this direction
   but cannot by itself establish generalized coding capability.
9. Runtime discovery inventories CPU, RAM, supported GPU devices and VRAM, the
   engine build and loadable acceleration backends, and whether requested
   offload is effective. It warns prominently on an unexpected CPU fallback or
   inert GPU-layer setting, chooses a conservative measured default for a
   compatible accelerated backend, and preserves an explicit operator
   override and provenance for every choice.
10. First-run calibration performs bounded warmup and capability probes before
    assigning a durable profile. It records prefill and decoded-token speed,
    memory/headroom, effective GPU offload, usable context, exact model/hash,
    quantization, engine build, hardware fingerprint class, and calibration
    timestamp; a profile is invalidated or requalified when those coordinates
    materially change.
11. Benchmark deadlines are positive, finite, speed-aware, and explicitly
    overridable. Effective per-task timeout scale, warmup provenance, output
    limit, and termination cause are retained so a backend-speed timeout is not
    silently graded as a model-capability failure.
12. Reasoning-capable models have an explicit profile or capability-probe
    result. Reasoning, visible response, structured action, action payload, and
    runaway-safety budgets are distinguishable and recorded; large write/edit
    actions receive context-bounded headroom without allowing unbounded output,
    and family-name inference alone does not silently select thinking policy.
13. Context, tier, and model-profile calibration distinguish trained context
    from locally usable context, include KV-cache and host-memory cost, and use
    parameter/family priors only as a transparent fallback. Measured capability
    may override the prior in either direction without losing the source of the
    decision.
14. Compaction has a separately tuned, bounded profile: its thinking behavior,
    output cap, temperature, trigger threshold, and reserved continuation
    budget are explicit and informed by usable context and measured speed.
    Compaction must preserve recovery/evidence semantics and report when it
    cannot produce a trustworthy continuation summary.

## Rationale

Sprint 113 showed that the existing Qwen2.5-Coder-7B control and an
evidence-bound controller both completed zero of three frozen long-horizon
tasks. The user asked for the next practical question: whether a stronger
current model that genuinely fits this machine can complete a realistic app
through Ferric, and whether Ferric's recently advertised Sprint Loops support
is operational rather than nominal. A pinned, instrumented trial answers both
without reopening the falsified Sprint 113 intervention.

The 2026-08-29 external refactor report adds a promising but narrower signal:
one Qwen3.8-27B run produced schema-valid actions and a working small app, while
CPU-only execution, reasoning-token pressure, static tier/context defaults,
and speed-blind benchmark deadlines distorted the surrounding evaluation. Its
recommendations are not implementation instructions; the acceptance criteria
above independently promote the product outcomes worth testing.

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

The expanded calibration contract adds startup probes and profile invalidation,
so first use is intentionally more deliberate. In return, later runs can reuse
an attributable profile instead of restating fragile flags or inheriting stale
constants. Accelerated backends remain optional capabilities: unsupported or
CPU-only hosts must receive truthful slower defaults rather than a false GPU
claim.

Explicit budget controls can ship as a diagnostic increment before automatic
calibration. Main-action output limits remain separate from reasoning and
compaction settings, and an explicit context-reserve check does not guarantee
tokenizer-accurate request fit or actual hardware capacity. A benchmark using a
non-default execution-time scale or an explicit output override must retain its
task results while marking calibration evidence ineligible, rather than
silently publishing a measured level under an old model/protocol-only profile.
Coordinate-bound calibration can lift that restriction once it represents and
requalifies those settings. Omitted defaults remain compatible; diagnostic
overrides must not enlarge tool authority or unrelated cleanup/grader deadlines.

## Transition history

- 2026-09-05: clarified the active intent's diagnostic budget boundary during
  Sprint 121 research. An explicit main-action output override or non-default
  benchmark execution-time scale may produce retained task observations, but
  must not publish or advertise a durable calibrated profile until the changed
  budget coordinates can be represented and requalified. Manual scaling is
  not automatic speed calibration; a declared context reserve is not measured
  model/hardware fit. State remains active and no new acceptance is claimed.

- 2026-08-26: created as `proposed` from the user's request for a
  hardware-fitting current model, a Ferric-built application, complete run
  evidence, and a live Animus Sprint Loops compatibility verdict.
- 2026-08-26: moved from `proposed` to `planned`; Sprint 114 tasks T-11407
  through T-11413 freeze model acquisition, sandboxed grading, managed-server
  calibration, the no-repair app trial, the layered skill probe, README
  cleanup, and truthful closeout.
- 2026-08-26: moved from `planned` to `active` when Sprint 114 Build began with
  the frozen Qwen3.8-27B quant acquisition task T-11407.
- 2026-08-29: Sprint 115 ended aborted with partial results. The exact CUDA runtime qualified at
  context 32,768 with 24 GPU layers and a 3.5651 decoded-token/s median, but
  its immutable live handoff ended externally before MH-RS01 began. The later
  exploratory counter application is encouraging but does not meet AC-3/4;
  T-11506, T-11410, and T-11412 remain ordered work and the intent stays
  active.
- 2026-08-30: expanded the active intent at the user's direction to preserve
  the constrained-JSON harness as the viable substrate while making backend
  acceleration, reasoning/action budgets, speed-aware benchmark deadlines,
  modern context/tier profiles, tuned compaction, and first-run calibration
  durable product requirements. The refactor report remains external evidence,
  not completion authority.
