# Sprint 122 — Research Report

## Sprint Goal

Give the interactive front door **hardware-informed model fit**: measure available
system memory, estimate each candidate GGUF's memory need, and surface an honest
per-model fit signal in the picker — replacing the blanket
`"Resource fit is not measured"` line with a real one and requiring a deliberate
confirmation before starting a model that will not fit.

This is a bounded first increment of **INT-0008 AC-13** ("hardware-informed
recommendations avoid asking users to choose quantization, acceleration flags or
paths") and advances **T-11507** ("explicit engine/GPU capability discovery …
without treating trained context as a hardware default"). It directly fixes the
first human use test's core failure: the picker offered a 15.3 GiB 27B, ran it on
CPU under "resource fit is not measured", and the session was unusable.

**Explicitly deferred (named so they do not evaporate):**
- In-product model/engine **acquisition/download** — AC-13's headline clause. It
  is network-heavy (fetch, checksum, progress, cancellation, interrupted
  recovery, license/consent) and touches the download-permission boundary; it is
  its own sprint and *depends on* this one for its "hardware-informed
  recommendation". Hardware fit is the prerequisite, so it comes first.
- **GPU/VRAM discovery and safe layer calibration** (the GPU half of T-11507).
  VRAM probing needs a vendor path (nvidia-smi / vulkan / metal) — a larger,
  platform-specific surface. The human-test failure was CPU RAM, so RAM fit is
  the load-bearing first cut; GPU fit is a clean follow-on.
- **T-12028** (duplicate `[[bin]]` build warning) — a separate build-config
  concern, not model fit.

## Existing Code Survey

- The picker already holds each model's size: `ModelChoice { label, bytes:
  Option<u64>, path }` (`crates/ferric-cli/src/startup.rs:34`). `choose_model`
  lists `bytes` as `"(X.X GiB file)"` (`crates/ferric-cli/src/human.rs:224`) but
  never compares it to anything.
- The only resource messaging is a blanket, pre-selection line:
  `"This starts a local CPU model and may use substantial memory. Resource fit is
  not measured."` (`crates/ferric-cli/src/human.rs:506`, gated on
  `start.will_start_engine`), and `"Loading the model with conservative CPU
  settings (not hardware-qualified)…"` (`crates/ferric-cli/src/startup.rs:443`).
- **No hardware capability discovery exists anywhere.** A grep for
  `sysinfo|total_memory|available_memory|VRAM|GlobalMemoryStatus|/proc/meminfo`
  across `crates/` returns nothing but the manual `gpu_layers` *config knob*
  (`server.rs:115`, ADR-045), which defaults to `0` (CPU) and is never inferred.
  So `"Resource fit is not measured"` is literally accurate — nothing measures it.
- The engine start is confirmed interactively at `human.rs:507` (`"Start the local
  model? [y/N]"`); this is the natural place to attach a fit-aware warning, and
  the picker at `human.rs:224` is the place to attach a per-model annotation.
- The `will_start_engine` flag (`startup.rs:165`) already distinguishes "we will
  actually load a model" from a reused/remote endpoint, so the probe runs only
  when it matters.

## External Sources

- **GGUF resident-memory rule of thumb (llama.cpp):** a model's RAM footprint ≈
  the on-disk file (weights are mmap'd/loaded near 1:1 for a given quant) **plus**
  the KV cache, which scales with context length and model dimensions. A defensible
  bounded heuristic is `file_bytes × headroom + context_allowance`, with the
  margin stated rather than precise — the goal is an honest *fits / tight /
  won't-fit* signal, not a byte-exact predictor.
- **Platform memory APIs, no new dependency required:**
  - Windows: `GlobalMemoryStatusEx` via `windows-sys` (**already a workspace dep**,
    `windows-sys = "0.61"`).
  - Linux: parse `/proc/meminfo` `MemAvailable` (std only).
  - macOS: `sysctl` `HW_MEMSIZE` via `libc` (**already a workspace dep**).
- ADR-004 dependency allowlist: this increment adds **no** new crate — a hard
  requirement, and satisfiable here.

## Risks / Unknowns / Dependencies

- **Estimate honesty over precision.** A wrong "fits" that then thrashes is worse
  than "unknown". The classifier must fail *open to "unmeasured"* when the probe
  returns nothing (a locked-down container, an unreadable `/proc`), and never
  fabricate a number. "Unknown" keeps today's behavior (warn, ask), so the change
  is strictly additive.
- **Available vs total memory.** `MemAvailable` / the Windows "avail phys" better
  reflect what a model can actually use than total; but they move moment to
  moment. Use available for the *warning* threshold, disclose the number, and keep
  the decision the user's.
- **Portability (ADR-004).** aarch64 Linux (RPi/Jetson) must still type-check and
  behave — `/proc/meminfo` covers Linux; the probe is behind a trait so the CI
  aarch64 gate and the non-Windows builds compile without platform drift.
- **Determinism for tests.** The estimator and the fits/tight/won't-fit classifier
  must be **pure functions** over `(file_bytes, context, available_bytes)`, and
  the OS probe behind a small trait, so unit tests need no real hardware — matching
  the codebase's `HumanIo`/`Preparation` seam style.
- **No scope creep into acquisition.** The warning must *inform*, not offer to
  download or change engine flags this sprint.

## Recommended Approach

1. **`MemoryProbe` seam** (best-effort available/total RAM): a trait with a native
   impl (`windows-sys` / `/proc/meminfo` / `libc`) returning
   `Option<SystemMemory>`; `None` is a valid, non-fabricated outcome.
2. **Pure fit model** in `ferric-core` (or a small `startup` submodule): a
   `estimate_model_memory(file_bytes, context)` and a
   `classify_fit(estimate, available) -> Fit { Fits, Tight, WontFit, Unknown }`,
   both pure and unit-tested, with the headroom margin a named constant and a
   doc-comment stating what it does and does not model.
3. **Honest surfaces:**
   - Picker (`human.rs:224`): annotate each listed model with its fit
     (`"… — likely fits"` / `"tight"` / `"needs ~N GiB, you have ~M"` / silent when
     unknown).
   - Engine-start gate (`human.rs:505-506`): replace the blanket line with the
     selected model's fit, and require an explicit extra confirmation when the fit
     is `WontFit`, naming the numbers — the guardrail the 27B-on-CPU run lacked.
4. **Tests:** pure estimator/classifier unit tests (incl. the exact 15.3 GiB-model
   / small-RAM case from the human test → `WontFit`); a probe-injected picker test
   asserting the annotation and the won't-fit confirmation; `Unknown` preserves
   current behavior. No new dependency; aarch64 check stays green.

## Intents Reviewed

- [INT-0008 — Unified local model workflow](../../intents/INT-0008-unified-local-model-workflow.md)
  — **selected and revised.** The owner added **AC-13** (in-product acquisition +
  hardware-informed recommendation → first conversation) in this sprint's Book
  prep. This sprint advances AC-13's *hardware-informed recommendation* clause via
  the bounded RAM-fit increment above; the acquisition clause is scoped as the
  named follow-on. No further intent text change is required by the research; the
  owner's AC-13 addition stands as the acceptance boundary.

## Referenced Artifacts

- This report: `docs/sprints/s122/sprint-research/research-report.md`
- Prior direction plan (context): `docs/plans/2026-09-06-direction-and-refactor.md`
- Intent: `docs/intents/INT-0008-unified-local-model-workflow.md` (AC-13)
